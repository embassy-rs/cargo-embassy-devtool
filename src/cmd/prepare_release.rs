use crate::bump::bump;
use crate::cmd::semver_check;
use crate::types::{Context, Crate};
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
        let start_crate = ctx
            .crates
            .get(crate_name)
            .expect("unable to find crate in tree");
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
            let deps = ctx.recursive_dependencies(std::iter::once(crate_name.as_str()));
            for dep_crate_name in deps {
                let c = ctx.crates.get(&dep_crate_name).unwrap();
                if c.publish && !to_bump.contains_key(&dep_crate_name) {
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
            let deps = ctx.recursive_dependencies(std::iter::once(name.as_str()));
            for dep_crate_name in deps {
                if let Some((ReleaseType::Patch, newver)) = to_bump.get(&dep_crate_name) {
                    let v = semver::Version::parse(newver)?;
                    let newver = semver::Version::new(v.major, v.minor + 1, 0);
                    to_bump.insert(
                        dep_crate_name.clone(),
                        (ReleaseType::Minor, newver.to_string()),
                    );
                }
            }
        }
    }

    for (name, (_, newver)) in to_bump.iter() {
        bump(ctx, name, newver)?;
    }

    for crate_name in crate_names {
        let c = ctx
            .crates
            .get(crate_name)
            .expect("unable to find crate in tree");
        publish_release(&ctx.root, c, false)?;
    }

    println!("# Please inspect changes and run the following commands when happy:");
    println!("git commit -a -m 'chore: prepare crate releases'");
    println!();
    let mut processed = HashSet::new();
    for crate_name in crate_names {
        let deps = ctx.recursive_dependencies(std::iter::once(crate_name.as_str()));
        for dep_crate_name in deps {
            let c = ctx.crates.get(&dep_crate_name).unwrap();
            if c.publish && !processed.contains(&dep_crate_name) {
                processed.insert(dep_crate_name.clone());
                println!("git tag {}-v{}", dep_crate_name, c.version);
            }
        }
    }
    let mut processed = HashSet::new();
    println!();
    println!("# Run these commands to publish the crate and dependents:");
    for crate_name in crate_names {
        let deps = ctx.recursive_dependencies(std::iter::once(crate_name.as_str()));
        for dep_crate_name in deps {
            if !processed.contains(&dep_crate_name) {
                processed.insert(dep_crate_name.clone());
                let c = ctx.crates.get(&dep_crate_name).unwrap();
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
