use std::collections::{BTreeMap, HashMap};
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{anyhow, Result};
use clap::{Parser, Subcommand};
use petgraph::graph::{Graph, NodeIndex};
use petgraph::visit::Bfs;
use petgraph::Directed;
use simple_logger::SimpleLogger;
use toml_edit::{DocumentMut, Item, Value};
use types::{Context, GraphContext, *};

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
    SetVersion(cmd::set_version::Args),
    Build(cmd::build::Args),
    SemverCheck(cmd::semver_check::Args),
    PrepareRelease(cmd::prepare_release::Args),
}

fn update_version(c: &mut Crate, new_version: &str) -> Result<()> {
    let path = c.path.join("Cargo.toml");
    c.version = new_version.to_string();
    let content = fs::read_to_string(&path)?;
    let mut doc: DocumentMut = content.parse()?;
    for section in ["package"] {
        if let Some(Item::Table(dep_table)) = doc.get_mut(section) {
            dep_table.insert("version", Item::Value(Value::from(new_version)));
        }
    }
    fs::write(&path, doc.to_string())?;
    Ok(())
}

fn update_versions(to_update: &Crate, dep: &CrateId, new_version: &str) -> Result<()> {
    let path = to_update.path.join("Cargo.toml");
    let content = fs::read_to_string(&path)?;
    let mut doc: DocumentMut = content.parse()?;
    let mut changed = false;
    for section in ["dependencies", "dev-dependencies", "build-dependencies"] {
        if let Some(Item::Table(dep_table)) = doc.get_mut(section) {
            if let Some(item) = dep_table.get_mut(dep) {
                match item {
                    // e.g., foo = "0.1.0"
                    Item::Value(Value::String(_)) => {
                        *item = Item::Value(Value::from(new_version));
                        changed = true;
                    }
                    // e.g., foo = { version = "...", ... }
                    Item::Value(Value::InlineTable(inline)) => {
                        if inline.contains_key("version") {
                            inline["version"] = Value::from(new_version);
                            changed = true;
                        }
                    }
                    _ => {} // Leave unusual formats untouched
                }
            }
        }
    }

    if changed {
        fs::write(&path, doc.to_string())?;
        println!(
            "🔧 Updated {} to {} in {}",
            dep,
            new_version,
            path.display()
        );
    }
    Ok(())
}

fn list_crates(root: &PathBuf) -> Result<BTreeMap<CrateId, Crate>> {
    let mut crates = BTreeMap::new();
    discover_crates(root, &mut crates)?;
    Ok(crates)
}

fn discover_crates(dir: &PathBuf, crates: &mut BTreeMap<CrateId, Crate>) -> Result<()> {
    let wd = walkdir::WalkDir::new(dir);
    for entry in wd
        .into_iter()
        .filter_entry(|e| e.file_type().is_dir() && !e.file_name().eq_ignore_ascii_case("target"))
    {
        let entry = entry?;
        let path = dir.join(entry.path());
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
                for (k, _) in parsed.dependencies {
                    if k.starts_with("embassy-") {
                        dependencies.push(k);
                    }
                }

                let mut dev_dependencies = Vec::new();
                if let Some(deps) = parsed.dev_dependencies {
                    for (k, _) in deps {
                        if k.starts_with("embassy-") {
                            dev_dependencies.push(k);
                        }
                    }
                }

                let mut build_dependencies = Vec::new();
                if let Some(deps) = parsed.build_dependencies {
                    for (k, _) in deps {
                        if k.starts_with("embassy-") {
                            build_dependencies.push(k);
                        }
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
                    },
                );
            }
        }
    }
    Ok(())
}

fn build_graph(
    crates: &BTreeMap<CrateId, Crate>,
    select_deps: impl Fn(&Crate) -> &Vec<CrateId>,
) -> GraphContext {
    let mut graph = Graph::<CrateId, (), Directed>::new();
    let mut node_indices: HashMap<CrateId, NodeIndex> = HashMap::new();

    // Helper to insert or get existing node
    let get_or_insert_node =
        |id: CrateId, graph: &mut Graph<CrateId, ()>, map: &mut HashMap<CrateId, NodeIndex>| {
            if let Some(&idx) = map.get(&id) {
                idx
            } else {
                let idx = graph.add_node(id.clone());
                map.insert(id, idx);
                idx
            }
        };

    for krate in crates.values() {
        get_or_insert_node(krate.name.clone(), &mut graph, &mut node_indices);
    }

    for krate in crates.values() {
        // Insert crate node if not exists
        let crate_idx = get_or_insert_node(krate.name.clone(), &mut graph, &mut node_indices);

        // Insert dependencies and connect edges
        for dep in select_deps(krate).iter() {
            let dep_idx = get_or_insert_node(dep.clone(), &mut graph, &mut node_indices);
            graph.add_edge(crate_idx, dep_idx, ());
        }
    }

    graph.reverse();
    GraphContext {
        g: graph,
        i: node_indices,
    }
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
    let graph = build_graph(&crates, |c| &c.dependencies);
    let dev_graph = build_graph(&crates, |c| &c.dev_dependencies);
    let build_graph = build_graph(&crates, |c| &c.build_dependencies);

    let ctx = Context {
        root,
        crates,
        graph,
        dev_graph,
        build_graph,
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
        Command::SetVersion(args) => {
            cmd::set_version::run(&mut ctx, args)?;
        }
        Command::SemverCheck(args) => {
            cmd::semver_check::run(&ctx, args)?;
        }
        Command::PrepareRelease(args) => {
            cmd::prepare_release::run(&mut ctx, args)?;
        }
    }
    Ok(())
}

fn update_graph_deps(
    ctx: &Context,
    graph: &GraphContext,
    name: &CrateId,
    oldver: &str,
    newver: &str,
) -> Result<(), anyhow::Error> {
    let node = graph.i.get(name).expect("unable to find crate in tree");
    let mut bfs = Bfs::new(&graph.g, *node);
    while let Some(dep_node) = bfs.next(&graph.g) {
        let dep_weight = graph.g.node_weight(dep_node).unwrap();
        println!("Updating {name}-{oldver} -> {newver} for {dep_weight}");
        let dep = ctx.crates.get(dep_weight).unwrap();
        update_versions(dep, name, newver)?;
    }
    Ok(())
}

fn update_changelog(repo: &Path, c: &Crate) -> Result<()> {
    let args: Vec<String> = vec![
        "release".to_string(),
        "replace".to_string(),
        "--config".to_string(),
        repo.join("release")
            .join("release.toml")
            .display()
            .to_string(),
        "--manifest-path".to_string(),
        c.path.join("Cargo.toml").display().to_string(),
        "--execute".to_string(),
        "--no-confirm".to_string(),
    ];

    let status = std::process::Command::new("cargo").args(&args).output()?;

    println!("{}", core::str::from_utf8(&status.stdout).unwrap());
    eprintln!("{}", core::str::from_utf8(&status.stderr).unwrap());
    if !status.status.success() {
        Err(anyhow!("release replace failed"))
    } else {
        Ok(())
    }
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
