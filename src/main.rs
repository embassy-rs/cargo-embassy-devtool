use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command as ProcessCommand;

use anyhow::{anyhow, bail, Result};
use cargo_semver_checks::ReleaseType;
use clap::{Parser, Subcommand};
use petgraph::graph::{Graph, NodeIndex};
use petgraph::visit::Bfs;
use petgraph::{Directed, Direction};
use simple_logger::SimpleLogger;
use toml_edit::{DocumentMut, Item, Value};
use types::*;

mod build;
mod cargo;
mod semver_check;
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
    /// All crates and their direct dependencies
    List,
    /// List all dependencies for a crate
    Dependencies {
        /// Crate name to print dependencies for.
        #[arg(value_name = "CRATE")]
        crate_name: String,
    },
    /// List all dependencies for a crate
    Dependents {
        /// Crate name to print dependencies for.
        #[arg(value_name = "CRATE")]
        crate_name: String,
    },
    /// Force set a dependency to a version.
    ///
    /// Can be used to override result of prepare release
    SetVersion {
        /// Crate name to print dependencies for.
        #[arg(value_name = "CRATE")]
        crate_name: String,

        #[arg(value_name = "CRATE_VERSION")]
        crate_version: String,
    },

    /// Build
    Build {
        /// Crate to check. If not specified checks all crates.
        #[arg(value_name = "CRATE")]
        crate_name: Option<String>,
    },
    /// SemverCheck
    SemverCheck {
        /// Specific crate name to check
        #[arg(value_name = "CRATE")]
        crate_name: String,
    },
    /// Prepare to release crates and all dependents that needs updating
    /// - Semver checks
    /// - Bump versions and commit
    /// - Create tag.
    PrepareRelease {
        /// Crates to release. Will traverse that crate an it's dependents. If not specified checks all crates.
        /// Crates specified in this list must be diseparate in the dependency tree
        #[arg(value_name = "CRATES")]
        crate_names: Vec<String>,
    },
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
                    configs.push(BuildConfig {
                        features: vec![],
                        target: None,
                        artifact_dir: None,
                    })
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

struct Context {
    root: PathBuf,
    crates: BTreeMap<String, Crate>,
    graph: GraphContext,
    dev_graph: GraphContext,
    build_graph: GraphContext,
}

struct GraphContext {
    g: Graph<String, ()>,
    i: HashMap<String, NodeIndex>,
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
        Command::List => {
            let ordered = petgraph::algo::toposort(&ctx.graph.g, None).unwrap();
            for node in ordered.iter() {
                let start = ctx.graph.g.node_weight(*node).unwrap();
                let mut bfs = Bfs::new(&ctx.graph.g, *node);
                while let Some(node) = bfs.next(&ctx.graph.g) {
                    let weight = ctx.graph.g.node_weight(node).unwrap();
                    let c = ctx.crates.get(weight).unwrap();
                    if weight == start {
                        println!("+ {}-{}", weight, c.version);
                    } else {
                        println!("|- {}-{}", weight, c.version);
                    }
                }
                println!();
            }
        }
        Command::Dependencies { crate_name } => {
            let idx = ctx
                .graph
                .i
                .get(&crate_name)
                .expect("unable to find crate in tree");
            let mut bfs = Bfs::new(&ctx.graph.g, *idx);
            while let Some(node) = bfs.next(&ctx.graph.g) {
                let weight = ctx.graph.g.node_weight(node).unwrap();
                let crt = ctx.crates.get(weight).unwrap();
                if *weight == crate_name {
                    println!("+ {}-{}", weight, crt.version);
                } else {
                    println!("|- {}-{}", weight, crt.version);
                }
            }
        }
        Command::Dependents { crate_name } => {
            let idx = ctx
                .graph
                .i
                .get(&crate_name)
                .expect("unable to find crate in tree");
            let weight = ctx.graph.g.node_weight(*idx).unwrap();
            let crt = ctx.crates.get(weight).unwrap();
            println!("+ {}-{}", weight, crt.version);
            for parent in ctx.graph.g.neighbors_directed(*idx, Direction::Incoming) {
                let weight = ctx.graph.g.node_weight(parent).unwrap();
                let crt = ctx.crates.get(weight).unwrap();
                println!("|- {}-{}", weight, crt.version);
            }
        }
        Command::Build { crate_name } => {
            build::build(&ctx, crate_name.as_deref())?;
        }
        Command::SetVersion {
            crate_name,
            crate_version,
        } => {
            let newver = crate_version;
            let name = &crate_name;
            let c = ctx.crates.get_mut(name).unwrap();
            let oldver = c.version.clone();
            update_version(c, &newver)?;

            let c = ctx.crates.get(name).unwrap();
            // Update all nodes further down the tree
            update_graph_deps(&ctx, &ctx.graph, name, &oldver, &newver)?;
            update_graph_deps(&ctx, &ctx.build_graph, name, &oldver, &newver)?;
            update_graph_deps(&ctx, &ctx.dev_graph, name, &oldver, &newver)?;

            // Update changelog
            update_changelog(&ctx.root, c)?;
        }
        Command::SemverCheck { crate_name } => {
            let c = ctx.crates.get(&crate_name).unwrap();
            if !c.publish {
                bail!(
                    "Cannot run semver-check on non-publishable crate '{}'",
                    crate_name
                );
            }
            check_semver(ctx.root.clone(), c)?;
        }
        Command::PrepareRelease { crate_names } => {
            // Check if the target crates are publishable
            for crate_name in &crate_names {
                let start = ctx
                    .graph
                    .i
                    .get(crate_name)
                    .expect("unable to find crate in tree");
                let start_weight = ctx.graph.g.node_weight(*start).unwrap();
                let start_crate = ctx.crates.get(start_weight).unwrap();
                if !start_crate.publish {
                    bail!(
                        "Cannot prepare release for non-publishable crate '{}'",
                        crate_name
                    );
                }
            }

            let mut to_bump = HashMap::new();
            // Do semver checks to figure out which versions to bump
            for crate_name in &crate_names {
                if !to_bump.contains_key(crate_name) {
                    let start = ctx
                        .graph
                        .i
                        .get(crate_name)
                        .expect("unable to find crate in tree");

                    let mut bfs = Bfs::new(&ctx.graph.g, *start);
                    while let Some(node) = bfs.next(&ctx.graph.g) {
                        let weight = ctx.graph.g.node_weight(node).unwrap();
                        println!("Preparing {weight}");
                        let c = ctx.crates.get(weight).unwrap();
                        if c.publish && !to_bump.contains_key(weight) {
                            let ver = semver::Version::parse(&c.version)?;
                            let (rtype, newver) = match check_semver(ctx.root.clone(), c)? {
                                ReleaseType::Major | ReleaseType::Minor => (
                                    ReleaseType::Minor,
                                    semver::Version::new(ver.major, ver.minor + 1, 0),
                                ),
                                ReleaseType::Patch => (
                                    ReleaseType::Patch,
                                    semver::Version::new(ver.major, ver.minor, ver.patch + 1),
                                ),
                                _ => unreachable!(),
                            };

                            println!("Updating {} from {} -> {}", weight, c.version, newver);
                            let newver = newver.to_string();
                            to_bump.insert(c.name.clone(), (rtype, newver.clone()));
                        }
                    }
                }
            }

            // Ensure minor bumps get propagated
            let keys: Vec<String> = to_bump.keys().map(|s| s.clone()).collect();
            for name in keys {
                let (rtype, _) = to_bump[&name];

                if rtype == ReleaseType::Minor {
                    let start = ctx
                        .graph
                        .i
                        .get(&name)
                        .expect("unable to find crate in tree");

                    let mut bfs = Bfs::new(&ctx.graph.g, *start);
                    while let Some(node) = bfs.next(&ctx.graph.g) {
                        let weight = ctx.graph.g.node_weight(node).unwrap();
                        if let Some((ReleaseType::Patch, newver)) = to_bump.get(weight) {
                            let v = semver::Version::parse(newver)?;
                            let newver = semver::Version::new(v.major, v.minor + 1, 0);
                            to_bump
                                .insert(weight.clone(), (ReleaseType::Minor, newver.to_string()));
                        }
                    }
                }
            }

            // Bump the versions as agreed
            for (name, (_, newver)) in to_bump.iter() {
                let c = ctx.crates.get_mut(name).unwrap();
                let oldver = c.version.clone();
                update_version(c, &newver)?;
                let c = ctx.crates.get(name).unwrap();

                // Update all nodes further down the tree
                update_graph_deps(&ctx, &ctx.graph, name, &oldver, &newver)?;
                update_graph_deps(&ctx, &ctx.build_graph, name, &oldver, &newver)?;
                update_graph_deps(&ctx, &ctx.dev_graph, name, &oldver, &newver)?;

                // Update changelog
                update_changelog(&ctx.root, c)?;
            }

            // Test publish
            for crate_name in &crate_names {
                let start = ctx
                    .graph
                    .i
                    .get(crate_name)
                    .expect("unable to find crate in tree");
                let weight = ctx.graph.g.node_weight(*start).unwrap();
                let c = ctx.crates.get(weight).unwrap();
                publish_release(&ctx.root, c, false)?;
            }

            println!("# Please inspect changes and run the following commands when happy:");
            println!("git commit -a -m 'chore: prepare crate releases'");
            println!();

            let mut processed = HashSet::new();
            for crate_name in &crate_names {
                let start = ctx
                    .graph
                    .i
                    .get(crate_name)
                    .expect("unable to find crate in tree");
                let mut bfs = Bfs::new(&ctx.graph.g, *start);
                while let Some(node) = bfs.next(&ctx.graph.g) {
                    let weight = ctx.graph.g.node_weight(node).unwrap();
                    let c = ctx.crates.get(weight).unwrap();
                    if c.publish && !processed.contains(weight) {
                        processed.insert(weight.clone());
                        println!("git tag {}-v{}", weight, c.version);
                    }
                }
            }

            let mut processed = HashSet::new();
            println!();
            println!("# Run these commands to publish the crate and dependents:");
            for crate_name in &crate_names {
                let start = ctx
                    .graph
                    .i
                    .get(crate_name)
                    .expect("unable to find crate in tree");
                let mut bfs = Bfs::new(&ctx.graph.g, *start);
                while let Some(node) = bfs.next(&ctx.graph.g) {
                    let weight = ctx.graph.g.node_weight(node).unwrap();

                    if !processed.contains(weight) {
                        processed.insert(weight.clone());
                        let c = ctx.crates.get(weight).unwrap();

                        let mut args: Vec<String> = vec![
                            "publish".to_string(),
                            "--manifest-path".to_string(),
                            c.path.join("Cargo.toml").display().to_string(),
                        ];

                        let config = c.configs.first().unwrap(); // TODO
                        if !config.features.is_empty() {
                            args.push("--features".into());
                            args.push(config.features.join(","));
                        }

                        if let Some(target) = &config.target {
                            args.push("--target".into());
                            args.push(target.clone());
                        }

                        /*
                        let mut dry_run = args.clone();
                        dry_run.push("--dry-run".to_string());

                        println!("cargo {}", dry_run.join(" "));
                        */
                        if c.publish {
                            println!("cargo {}", args.join(" "));
                        }
                    }
                }
            }

            println!();
            println!("# Run this command to push changes and tags:");
            println!("git push --tags");
        }
    }
    Ok(())
}

fn check_semver(root: PathBuf, c: &Crate) -> Result<ReleaseType> {
    let min_version = semver_check::minimum_update(root, c)?;
    println!("Version should be bumped to {min_version:?}");
    Ok(min_version)
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
        println!(
            "Updating {}-{} -> {} for {}",
            name, oldver, newver, dep_weight
        );
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

    let status = ProcessCommand::new("cargo").args(&args).output()?;

    println!("{}", core::str::from_utf8(&status.stdout).unwrap());
    eprintln!("{}", core::str::from_utf8(&status.stderr).unwrap());
    if !status.status.success() {
        Err(anyhow!("release replace failed"))
    } else {
        Ok(())
    }
}

fn publish_release(_repo: &Path, c: &Crate, push: bool) -> Result<()> {
    let config = c.configs.first().unwrap();
    let mut args: Vec<String> = vec![
        "publish".to_string(),
        "--manifest-path".to_string(),
        c.path.join("Cargo.toml").display().to_string(),
    ];

    args.push("--features".into());
    args.push(config.features.join(","));

    if let Some(target) = &config.target {
        args.push("--target".into());
        args.push(target.clone());
    }

    if !push {
        args.push("--dry-run".to_string());
        args.push("--allow-dirty".to_string());
        args.push("--keep-going".to_string());
    }

    let status = ProcessCommand::new("cargo").args(&args).output()?;

    println!("{}", core::str::from_utf8(&status.stdout).unwrap());
    eprintln!("{}", core::str::from_utf8(&status.stderr).unwrap());
    if !status.status.success() {
        Err(anyhow!("publish failed"))
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
