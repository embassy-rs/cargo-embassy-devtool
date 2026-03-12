use crate::bump::bump;
use crate::cmd::semver_check;
use crate::types::{Context, Crate, CrateId};
use anyhow::{Result, anyhow, bail};
use cargo_semver_checks::ReleaseType;
use std::collections::HashMap;
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

/// The release plan computed from semver analysis.
#[derive(Debug)]
pub struct ReleasePlan {
    /// All crates that need a version bump, with their release type and new version.
    pub to_bump: HashMap<CrateId, (ReleaseType, String)>,
}

impl ReleasePlan {
    /// Returns whether a crate should be bumped, tagged, and published.
    /// Only crates present in `to_bump` are released: explicitly requested
    /// crates and dependents that require a Minor or Major bump.
    pub fn should_release(&self, name: &str) -> bool {
        self.to_bump.contains_key(name)
    }
}

/// Compute the release plan: which crates need bumping and to what version.
/// The `check_semver_fn` closure allows injecting semver check results for testing.
pub fn plan_release<F>(ctx: &Context, crate_names: &[String], mut check_semver_fn: F) -> Result<ReleasePlan>
where
    F: FnMut(&Crate) -> Result<ReleaseType>,
{
    let mut to_bump = HashMap::new();
    for crate_name in crate_names {
        if !to_bump.contains_key(crate_name) {
            let deps = ctx.recursive_dependents(std::iter::once(crate_name.as_str()));
            for dep_crate_name in deps {
                let c = ctx.crates.get(&dep_crate_name).unwrap();
                if c.publish && !to_bump.contains_key(&dep_crate_name) {
                    let ver = semver::Version::parse(&c.version)?;
                    let (rtype, newver) = match check_semver_fn(c)? {
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

    // Propagate minor bumps: if a crate gets a minor bump, all its dependents
    // that only had patch bumps are upgraded to minor.
    let keys: Vec<String> = to_bump.keys().cloned().collect();
    for name in keys {
        let (rtype, _) = to_bump[&name];
        if rtype == ReleaseType::Minor {
            let deps = ctx.recursive_dependents(std::iter::once(name.as_str()));
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

    // Remove patch-only dependents: they don't need a version bump since
    // they won't be released. Only keep explicitly requested crates and
    // crates that require a minor or major bump.
    to_bump.retain(|name, (rtype, _)| {
        crate_names.contains(name)
            || matches!(rtype, ReleaseType::Minor | ReleaseType::Major)
    });

    Ok(ReleasePlan { to_bump })
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

    let plan = plan_release(ctx, crate_names, |c| {
        semver_check::check_semver(ctx.root.clone(), c)
    })?;

    for (name, (_, newver)) in plan.to_bump.iter() {
        if plan.should_release(name) {
            bump(ctx, name, newver)?;
        }
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
    for crate_name in crate_names {
        let deps = ctx.recursive_dependents(std::iter::once(crate_name.as_str()));
        for dep_crate_name in deps {
            if plan.should_release(&dep_crate_name) {
                let c = ctx.crates.get(&dep_crate_name).unwrap();
                println!("git tag {}-v{}", dep_crate_name, c.version);
            }
        }
    }
    println!();
    println!("# Run these commands to publish the crate and dependents:");
    for crate_name in crate_names {
        let deps = ctx.recursive_dependents(std::iter::once(crate_name.as_str()));
        for dep_crate_name in deps {
            if plan.should_release(&dep_crate_name) {
                let c = ctx.crates.get(&dep_crate_name).unwrap();
                if c.publish {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{BuildConfig, Context, Crate};
    use std::collections::{BTreeMap, HashMap, HashSet};
    use std::path::PathBuf;

    fn make_crate(name: &str, version: &str, deps: Vec<&str>, publish: bool) -> Crate {
        Crate {
            name: name.to_string(),
            version: version.to_string(),
            path: PathBuf::from(format!("/tmp/{}", name)),
            dependencies: deps.into_iter().map(|s| s.to_string()).collect(),
            build_dependencies: vec![],
            dev_dependencies: vec![],
            configs: vec![BuildConfig::default()],
            publish,
            doc: false,
        }
    }

    fn make_context(crates: Vec<Crate>) -> Context {
        let mut crate_map = BTreeMap::new();
        let mut reverse_deps: HashMap<String, HashSet<String>> = HashMap::new();

        for c in &crates {
            for dep in c.all_dependencies() {
                reverse_deps
                    .entry(dep.clone())
                    .or_default()
                    .insert(c.name.clone());
            }
        }

        for c in crates {
            crate_map.insert(c.name.clone(), c);
        }

        Context {
            root: PathBuf::from("/tmp/repo"),
            crates: crate_map,
            reverse_deps,
        }
    }

    #[test]
    fn explicit_patch_crate_is_released() {
        // A crate explicitly requested for release should be released even with a patch bump
        let ctx = make_context(vec![
            make_crate("embassy-a", "0.5.0", vec![], true),
        ]);

        let plan = plan_release(&ctx, &["embassy-a".to_string()], |_| {
            Ok(ReleaseType::Patch)
        })
        .unwrap();

        assert!(plan.should_release("embassy-a"));
        assert_eq!(plan.to_bump["embassy-a"].1, "0.5.1");
    }

    #[test]
    fn patch_dependent_not_released() {
        // When A gets a patch bump, dependent B should NOT be released
        let ctx = make_context(vec![
            make_crate("embassy-a", "0.5.0", vec![], true),
            make_crate("embassy-b", "0.3.0", vec!["embassy-a"], true),
        ]);

        let crate_names = vec!["embassy-a".to_string()];
        let plan = plan_release(&ctx, &crate_names, |_| Ok(ReleaseType::Patch)).unwrap();

        assert!(plan.should_release("embassy-a"));
        assert_eq!(plan.to_bump["embassy-a"].1, "0.5.1");
        assert!(!plan.should_release("embassy-b"));
        assert!(!plan.to_bump.contains_key("embassy-b"));
    }

    #[test]
    fn minor_dependent_is_released() {
        // When A gets a minor bump, dependent B (also minor via propagation) should be released
        let ctx = make_context(vec![
            make_crate("embassy-a", "0.5.0", vec![], true),
            make_crate("embassy-b", "0.3.0", vec!["embassy-a"], true),
        ]);

        let crate_names = vec!["embassy-a".to_string()];
        let plan = plan_release(&ctx, &crate_names, |_| Ok(ReleaseType::Minor)).unwrap();

        assert!(plan.should_release("embassy-a"));
        assert!(plan.should_release("embassy-b"));
        assert_eq!(plan.to_bump["embassy-a"].0, ReleaseType::Minor);
        assert_eq!(plan.to_bump["embassy-a"].1, "0.6.0");
        assert_eq!(plan.to_bump["embassy-b"].0, ReleaseType::Minor);
        assert_eq!(plan.to_bump["embassy-b"].1, "0.4.0");
    }

    #[test]
    fn minor_propagates_patch_to_minor() {
        // If A gets minor and B (dependent of A) gets patch from semver check,
        // B should be upgraded to minor via propagation
        let ctx = make_context(vec![
            make_crate("embassy-a", "0.5.0", vec![], true),
            make_crate("embassy-b", "0.3.0", vec!["embassy-a"], true),
        ]);

        let crate_names = vec!["embassy-a".to_string()];
        let plan = plan_release(&ctx, &crate_names, |c| {
            if c.name == "embassy-a" {
                Ok(ReleaseType::Minor)
            } else {
                Ok(ReleaseType::Patch)
            }
        })
        .unwrap();

        // B's patch should be promoted to minor
        assert_eq!(plan.to_bump["embassy-b"].0, ReleaseType::Minor);
        assert_eq!(plan.to_bump["embassy-b"].1, "0.4.0");
        assert!(plan.should_release("embassy-b"));
    }

    #[test]
    fn major_treated_as_minor() {
        // Major bumps should be mapped to Minor
        let ctx = make_context(vec![
            make_crate("embassy-a", "0.5.0", vec![], true),
        ]);

        let crate_names = vec!["embassy-a".to_string()];
        let plan = plan_release(&ctx, &crate_names, |_| Ok(ReleaseType::Major)).unwrap();

        assert_eq!(plan.to_bump["embassy-a"].0, ReleaseType::Minor);
        assert_eq!(plan.to_bump["embassy-a"].1, "0.6.0");
    }

    #[test]
    fn non_publishable_dependents_excluded() {
        // Non-publishable dependents should not appear in the plan
        let ctx = make_context(vec![
            make_crate("embassy-a", "0.5.0", vec![], true),
            make_crate("embassy-b", "0.3.0", vec!["embassy-a"], false),
        ]);

        let crate_names = vec!["embassy-a".to_string()];
        let plan = plan_release(&ctx, &crate_names, |_| Ok(ReleaseType::Minor)).unwrap();

        assert!(!plan.to_bump.contains_key("embassy-b"));
    }

    #[test]
    fn transitive_patch_dependents_not_released() {
        // A (patch) -> B depends on A -> C depends on B
        // Neither B nor C should be released
        let ctx = make_context(vec![
            make_crate("embassy-a", "0.5.0", vec![], true),
            make_crate("embassy-b", "0.3.0", vec!["embassy-a"], true),
            make_crate("embassy-c", "0.1.0", vec!["embassy-b"], true),
        ]);

        let crate_names = vec!["embassy-a".to_string()];
        let plan = plan_release(&ctx, &crate_names, |_| Ok(ReleaseType::Patch)).unwrap();

        assert!(plan.should_release("embassy-a"));
        assert_eq!(plan.to_bump["embassy-a"].1, "0.5.1");
        assert!(!plan.should_release("embassy-b"));
        assert!(!plan.to_bump.contains_key("embassy-b"));
        assert!(!plan.should_release("embassy-c"));
        assert!(!plan.to_bump.contains_key("embassy-c"));
    }

    #[test]
    fn transitive_minor_propagation() {
        // A (minor) -> B depends on A -> C depends on B
        // Minor should propagate through the entire chain
        let ctx = make_context(vec![
            make_crate("embassy-a", "0.5.0", vec![], true),
            make_crate("embassy-b", "0.3.0", vec!["embassy-a"], true),
            make_crate("embassy-c", "0.1.0", vec!["embassy-b"], true),
        ]);

        let crate_names = vec!["embassy-a".to_string()];
        let plan = plan_release(&ctx, &crate_names, |c| {
            if c.name == "embassy-a" {
                Ok(ReleaseType::Minor)
            } else {
                Ok(ReleaseType::Patch)
            }
        })
        .unwrap();

        assert!(plan.should_release("embassy-a"));
        assert_eq!(plan.to_bump["embassy-a"].1, "0.6.0");
        assert!(plan.should_release("embassy-b"));
        assert_eq!(plan.to_bump["embassy-b"].0, ReleaseType::Minor);
        assert_eq!(plan.to_bump["embassy-b"].1, "0.4.0");
        assert!(plan.should_release("embassy-c"));
        assert_eq!(plan.to_bump["embassy-c"].0, ReleaseType::Minor);
        assert_eq!(plan.to_bump["embassy-c"].1, "0.2.0");
    }

    #[test]
    fn mixed_patch_and_minor_dependents() {
        // A (minor) -> B depends on A (patch, promoted to minor)
        // A (minor) -> C depends on A (independently minor)
        // D depends on neither A nor B nor C — should not appear
        let ctx = make_context(vec![
            make_crate("embassy-a", "0.5.0", vec![], true),
            make_crate("embassy-b", "0.3.0", vec!["embassy-a"], true),
            make_crate("embassy-c", "0.2.0", vec!["embassy-a"], true),
            make_crate("embassy-d", "0.1.0", vec![], true),
        ]);

        let crate_names = vec!["embassy-a".to_string()];
        let plan = plan_release(&ctx, &crate_names, |c| {
            if c.name == "embassy-a" || c.name == "embassy-c" {
                Ok(ReleaseType::Minor)
            } else {
                Ok(ReleaseType::Patch)
            }
        })
        .unwrap();

        assert!(plan.should_release("embassy-a"));
        assert_eq!(plan.to_bump["embassy-a"].1, "0.6.0");
        assert!(plan.should_release("embassy-b")); // promoted
        assert_eq!(plan.to_bump["embassy-b"].0, ReleaseType::Minor);
        assert_eq!(plan.to_bump["embassy-b"].1, "0.4.0");
        assert!(plan.should_release("embassy-c")); // own minor
        assert_eq!(plan.to_bump["embassy-c"].0, ReleaseType::Minor);
        assert_eq!(plan.to_bump["embassy-c"].1, "0.3.0");
        assert!(!plan.to_bump.contains_key("embassy-d")); // unrelated
    }
}
