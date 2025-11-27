use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{anyhow, Result};
use clap::{Parser, Subcommand};
use simple_logger::SimpleLogger;
use types::{Context, *};

mod bump;
mod cargo;
mod cmd;
mod types;

/// Tool to traverse and operate on intra-repo Rust crate dependencies
#[derive(Parser, Debug)]
#[command(author, version, about)]
struct Args {
    /// Command to perform on each crate
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    List(cmd::list::Args),
    Dependencies(cmd::dependencies::Args),
    Dependents(cmd::dependents::Args),
    Bump(cmd::bump::Args),
    Build(cmd::build::Args),
    SemverCheck(cmd::semver_check::Args),
    PrepareRelease(cmd::prepare_release::Args),
    CheckManifest(cmd::check_manifest::Args),
    CheckCrlf(cmd::check_crlf::Args),
    Doc(cmd::doc::Args),
}

fn list_crates(root: &PathBuf) -> Result<BTreeMap<CrateId, Crate>> {
    let mut crates = BTreeMap::new();
    let wd = walkdir::WalkDir::new(root);
    for entry in wd
        .into_iter()
        .filter_entry(|e| e.file_type().is_dir() && !e.file_name().eq_ignore_ascii_case("target"))
    {
        let entry = entry?;
        let path = root.join(entry.path());
        let cargo_toml = path.join("Cargo.toml");

        if cargo_toml.exists() {
            let content = fs::read_to_string(&cargo_toml)?;

            // Try to parse as a crate, skip if it's a workspace
            let parsed: Result<ParsedCrate, _> = toml::from_str(&content);
            if let Ok(parsed) = parsed {
                let id = parsed.package.name;

                let metadata = &parsed.package.metadata.embassy;

                if metadata.skip {
                    continue;
                }

                let mut dependencies = Vec::new();
                let mut dev_dependencies = Vec::new();
                let mut build_dependencies = Vec::new();

                for (k, _) in parsed.dependencies {
                    if k.starts_with("embassy-") || k.starts_with("cyw43") {
                        dependencies.push(k);
                    }
                }

                for (k, _) in parsed.dev_dependencies {
                    if k.starts_with("embassy-") || k.starts_with("cyw43") {
                        dev_dependencies.push(k);
                    }
                }

                for (k, _) in parsed.build_dependencies {
                    if k.starts_with("embassy-") || k.starts_with("cyw43") {
                        build_dependencies.push(k);
                    }
                }

                let mut configs = metadata.build.clone();
                if configs.is_empty() {
                    configs.push(BuildConfig::default())
                }

                crates.insert(
                    id.clone(),
                    Crate {
                        name: id,
                        version: parsed.package.version,
                        path,
                        dependencies,
                        dev_dependencies,
                        build_dependencies,
                        configs,
                        publish: parsed.package.publish,
                        doc: parsed.package.metadata.embassy_docs.is_some(),
                    },
                );
            }
        }
    }
    Ok(crates)
}

fn find_repo_root() -> Result<PathBuf> {
    let mut path = std::env::current_dir()?.canonicalize()?;

    loop {
        // Check if this directory contains a .git directory
        if path.join(".git").exists() {
            return Ok(path);
        }

        // Move to parent directory
        match path.parent() {
            Some(parent) => path = parent.to_path_buf(),
            None => break,
        }
    }

    Err(anyhow!(
        "Could not find repository root. Make sure you're running this tool from within the embassy repository."
    ))
}

fn load_context() -> Result<Context> {
    let root = find_repo_root()?;
    let crates = list_crates(&root)?;

    let mut reverse_deps: HashMap<String, HashSet<String>> = HashMap::new();

    for (crate_name, krate) in &crates {
        for dep_name in krate.all_dependencies() {
            reverse_deps
                .entry(dep_name.clone())
                .or_insert_with(HashSet::new)
                .insert(crate_name.clone());
        }
    }

    let ctx = Context {
        root,
        crates,
        reverse_deps,
    };

    // Check for publish dependency conflicts
    check_publish_dependencies(&ctx)?;

    Ok(ctx)
}

#[derive(Debug, Parser)]
#[command(name = "cargo")]
#[command(bin_name = "cargo")]
#[command(version, propagate_version = true)]
enum Cargo {
    EmbassyDevtool(Args),
}

fn main() -> Result<()> {
    SimpleLogger::new().init().unwrap();
    let Cargo::EmbassyDevtool(args) = Cargo::parse();
    let mut ctx = load_context()?;

    match args.command {
        Command::List(args) => {
            cmd::list::run(&ctx, args)?;
        }
        Command::Dependencies(args) => {
            cmd::dependencies::run(&ctx, args)?;
        }
        Command::Dependents(args) => {
            cmd::dependents::run(&ctx, args)?;
        }
        Command::Build(args) => {
            cmd::build::run(&ctx, args)?;
        }
        Command::Bump(args) => {
            cmd::bump::run(&mut ctx, args)?;
        }
        Command::SemverCheck(args) => {
            cmd::semver_check::run(&ctx, args)?;
        }
        Command::PrepareRelease(args) => {
            cmd::prepare_release::run(&mut ctx, args)?;
        }
        Command::CheckManifest(args) => {
            cmd::check_manifest::run(&ctx, args)?;
        }
        Command::CheckCrlf(args) => {
            cmd::check_crlf::run(&ctx, args)?;
        }
        Command::Doc(args) => {
            cmd::doc::run(&ctx, args)?;
        }
    }
    Ok(())
}

/// Make the path "Windows"-safe
pub fn windows_safe_path(path: &Path) -> PathBuf {
    PathBuf::from(path.to_str().unwrap().to_string().replace("\\\\?\\", ""))
}

fn check_publish_dependencies(ctx: &Context) -> Result<()> {
    for krate in ctx.crates.values() {
        if krate.publish {
            for dep_name in &krate.dependencies {
                if let Some(dep_crate) = ctx.crates.get(dep_name) {
                    if !dep_crate.publish {
                        return Err(anyhow!(
                            "Publishable crate '{}' depends on non-publishable crate '{}'. This is not allowed.",
                            krate.name,
                            dep_name
                        ));
                    }
                }
            }
        }
    }
    Ok(())
}
