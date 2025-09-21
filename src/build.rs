use anyhow::Result;
use std::collections::HashMap;

// ...existing code...
use crate::types::BuildConfigBatch;

pub(crate) fn build(
    ctx: &crate::Context,
    crate_name: Option<&str>,
    group: Option<&str>,
) -> Result<()> {
    // Process either specific crate or all crates
    let crates_to_build: Vec<_> = if let Some(name) = crate_name {
        // Build only the specified crate
        if let Some(krate) = ctx.crates.get(name) {
            vec![krate]
        } else {
            return Err(anyhow::anyhow!("Crate '{}' not found", name));
        }
    } else {
        // Build all crates
        ctx.crates.values().collect()
    };

    // Group build configurations by their batch properties
    let mut batch_groups: HashMap<BuildConfigBatch, Vec<(String, &crate::types::BuildConfig)>> =
        HashMap::new();

    for krate in crates_to_build {
        for config in &krate.configs {
            // only build matching group.
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

    // Execute a separate cargo batch for each group
    for (batch_config, configs) in batch_groups {
        // Build the cargo batch command arguments directly
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

        // Prepare environment variables, merging RUSTFLAGS if already set
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
