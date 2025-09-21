use crate::cmd::semver_check;
use crate::types::{Context, Crate};
use crate::{update_changelog, update_graph_deps, update_version};
use anyhow::{anyhow, bail, Result};
use cargo_semver_checks::ReleaseType;
use std::collections::HashSet;
use std::path::Path;

/// Prepare to release crates and all dependents that needs updating
/// - Semver checks
/// - Bump versions and commit
/// - Create tag.
#[derive(Debug, clap::Args)]
pub struct Args {
    /// Crates to release. Will traverse that crate an it's dependents. If not specified checks all crates.
    /// Crates specified in this list must be diseparate in the dependency tree
    #[arg(value_name = "CRATES")]
    pub crate_names: Vec<String>,
}

pub fn run(ctx: &mut Context, args: Args) -> Result<()> {
    let crate_names = &args.crate_names;
    for crate_name in crate_names {
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

    let mut to_bump = std::collections::HashMap::new();
    for crate_name in crate_names {
        if !to_bump.contains_key(crate_name) {
            let start = ctx
                .graph
                .i
                .get(crate_name)
                .expect("unable to find crate in tree");
            let mut bfs = petgraph::visit::Bfs::new(&ctx.graph.g, *start);
            while let Some(node) = bfs.next(&ctx.graph.g) {
                let weight = ctx.graph.g.node_weight(node).unwrap();
                let c = ctx.crates.get(weight).unwrap();
                if c.publish && !to_bump.contains_key(weight) {
                    let ver = semver::Version::parse(&c.version)?;
                    let (rtype, newver) = match semver_check::check_semver(ctx.root.clone(), c)? {
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
                    let newver = newver.to_string();
                    to_bump.insert(c.name.clone(), (rtype, newver));
                }
            }
        }
    }

    let keys: Vec<String> = to_bump.keys().cloned().collect();
    for name in keys {
        let (rtype, _) = to_bump[&name];
        if rtype == ReleaseType::Minor {
            let start = ctx
                .graph
                .i
                .get(&name)
                .expect("unable to find crate in tree");
            let mut bfs = petgraph::visit::Bfs::new(&ctx.graph.g, *start);
            while let Some(node) = bfs.next(&ctx.graph.g) {
                let weight = ctx.graph.g.node_weight(node).unwrap();
                if let Some((ReleaseType::Patch, newver)) = to_bump.get(weight) {
                    let v = semver::Version::parse(newver)?;
                    let newver = semver::Version::new(v.major, v.minor + 1, 0);
                    to_bump.insert(weight.clone(), (ReleaseType::Minor, newver.to_string()));
                }
            }
        }
    }

    for (name, (_, newver)) in to_bump.iter() {
        let c = ctx.crates.get_mut(name).unwrap();
        let oldver = c.version.clone();
        update_version(c, newver)?;
        let c = ctx.crates.get(name).unwrap();
        update_graph_deps(ctx, &ctx.graph, name, &oldver, newver)?;
        update_graph_deps(ctx, &ctx.build_graph, name, &oldver, newver)?;
        update_graph_deps(ctx, &ctx.dev_graph, name, &oldver, newver)?;
        update_changelog(&ctx.root, c)?;
    }

    for crate_name in crate_names {
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
    for crate_name in crate_names {
        let start = ctx
            .graph
            .i
            .get(crate_name)
            .expect("unable to find crate in tree");
        let mut bfs = petgraph::visit::Bfs::new(&ctx.graph.g, *start);
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
    for crate_name in crate_names {
        let start = ctx
            .graph
            .i
            .get(crate_name)
            .expect("unable to find crate in tree");
        let mut bfs = petgraph::visit::Bfs::new(&ctx.graph.g, *start);
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
                let config = c.configs.first().unwrap();
                if !config.features.is_empty() {
                    args.push("--features".into());
                    args.push(config.features.join(","));
                }
                if let Some(target) = &config.target {
                    args.push("--target".into());
                    args.push(target.clone());
                }
                if c.publish {
                    println!("cargo {}", args.join(" "));
                }
            }
        }
    }
    println!();
    println!("# Run this command to push changes and tags:");
    println!("git push --tags");
    Ok(())
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

    let status = std::process::Command::new("cargo").args(&args).output()?;

    println!("{}", core::str::from_utf8(&status.stdout).unwrap());
    eprintln!("{}", core::str::from_utf8(&status.stderr).unwrap());
    if !status.status.success() {
        Err(anyhow!("publish failed"))
    } else {
        Ok(())
    }
}
