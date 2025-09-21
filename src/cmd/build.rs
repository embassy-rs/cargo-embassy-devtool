use crate::types::Context;
use anyhow::{anyhow, Result};
use std::collections::HashMap;

/// Build
#[derive(Debug, clap::Args)]
pub struct Args {
    /// Crate to check. If not specified checks all crates.
    #[arg(value_name = "CRATE")]
    pub crate_name: Option<String>,
    /// Group name. If specified it'll build all configs matching it, if not specified it'll build all configs with no group set.
    #[arg(long)]
    pub group: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct BuildConfigBatch {
    pub env: std::collections::BTreeMap<String, String>,
    pub build_std: Vec<String>,
}

pub fn run(ctx: &Context, args: Args) -> Result<()> {
    let crate_name = args.crate_name.as_deref();
    let group = args.group.as_deref();
    let crates_to_build: Vec<_> = if let Some(name) = crate_name {
        if let Some(krate) = ctx.crates.get(name) {
            vec![krate]
        } else {
            return Err(anyhow!("Crate '{}' not found", name));
        }
    } else {
        ctx.crates.values().collect()
    };

    let mut batch_groups: HashMap<BuildConfigBatch, Vec<(String, &crate::types::BuildConfig)>> =
        HashMap::new();

    for krate in crates_to_build {
        for config in &krate.configs {
            if config.group.as_deref() != group {
                continue;
            }

            let batch_key = BuildConfigBatch {
                env: config.env.clone(),
                build_std: config.build_std.clone(),
            };

            let crate_path = format!("{}/Cargo.toml", krate.path.to_string_lossy());
            batch_groups
                .entry(batch_key)
                .or_default()
                .push((crate_path, config));
        }
    }

    for (batch_config, configs) in batch_groups {
        let mut batch_args = vec!["batch".to_string()];
        if !batch_config.build_std.is_empty() {
            batch_args.push(format!("-Zbuild-std={}", batch_config.build_std.join(",")));
        }

        for (manifest_path, config) in configs {
            let mut args = vec![
                "build".to_string(),
                "--release".to_string(),
                format!("--manifest-path={}", manifest_path),
            ];

            if let Some(ref target) = config.target {
                args.push(format!("--target={}", target));
            }
            if !config.features.is_empty() {
                args.push(format!("--features={}", config.features.join(",")));
            }
            if let Some(ref artifact_dir) = config.artifact_dir {
                args.push(format!("--artifact-dir={}", artifact_dir));
            }

            batch_args.push("---".to_string());
            batch_args.extend(args);
        }

        let mut final_env = batch_config.env.clone();
        if let Some(config_rustflags) = final_env.get("RUSTFLAGS") {
            if let Ok(existing_rustflags) = std::env::var("RUSTFLAGS") {
                if !existing_rustflags.is_empty() {
                    final_env.insert(
                        "RUSTFLAGS".to_string(),
                        format!("{} {}", existing_rustflags, config_rustflags),
                    );
                }
            }
        }

        crate::cargo::run_with_env(&batch_args, &ctx.root, &final_env, false)?;
    }

    Ok(())
}
