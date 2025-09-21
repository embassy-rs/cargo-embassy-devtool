use std::collections::HashSet;
use std::path::Path;
use std::path::PathBuf;

use anyhow::anyhow;
use cargo_semver_checks::{Check, GlobalConfig, ReleaseType, Rustdoc};
use flate2::read::GzDecoder;
use tar::Archive;

use crate::types::{BuildConfig, Crate};

/// Return the minimum required bump for the next release.
/// Even if nothing changed this will be [ReleaseType::Patch]
pub fn minimum_update(root: PathBuf, krate: &Crate) -> Result<ReleaseType, anyhow::Error> {
    let package_name = krate.name.clone();
    let baseline_path = download_baseline(&root, &package_name, &krate.version)?;
    let mut baseline_krate = krate.clone();
    baseline_krate.path = baseline_path.clone();

    // Compare features as it's not covered by semver-checks
    if compare_features(&baseline_krate, krate)? {
        return Ok(ReleaseType::Minor);
    }

    let mut min_required_update = ReleaseType::Patch;
    for config in krate.configs.iter() {
        //        std::fs::remove_dir_all(baseline_path.join("target"))?;
        let baseline_path = build_doc_json(&baseline_krate, config)?;
        let current_path = build_doc_json(krate, config)?;

        let baseline = Rustdoc::from_path(&baseline_path);
        let doc = Rustdoc::from_path(&current_path);
        let mut semver_check = Check::new(doc);
        semver_check.with_default_features();
        semver_check.set_baseline(baseline);
        semver_check.set_packages(vec![package_name.clone()]);
        semver_check.set_release_type(ReleaseType::Patch);
        let extra_current_features = config.features.clone();
        let extra_baseline_features = config.features.clone();
        semver_check.set_extra_features(extra_current_features, extra_baseline_features);
        if let Some(target) = &config.target {
            semver_check.set_build_target(target.clone());
        }
        let mut cfg = GlobalConfig::new();
        cfg.set_log_level(Some(log::Level::Info));

        let result = semver_check.check_release(&mut cfg)?;

        for report in result.crate_reports().values() {
            if let Some(required_bump) = report.required_bump() {
                let required_is_stricter = (min_required_update == ReleaseType::Patch)
                    || (required_bump == ReleaseType::Major);
                if required_is_stricter {
                    min_required_update = required_bump;
                }
            }
        }
    }

    Ok(min_required_update)
}

fn compare_features(old: &Crate, new: &Crate) -> Result<bool, anyhow::Error> {
    let mut old = read_features(&old.path)?;
    let new = read_features(&new.path)?;

    old.retain(|r| !new.contains(r));
    log::info!("Features removed in new: {old:?}");
    Ok(!old.is_empty())
}

fn download_baseline(root: &Path, name: &str, version: &str) -> Result<PathBuf, anyhow::Error> {
    let config = crates_index::IndexConfig {
        dl: "https://crates.io/api/v1/crates".to_string(),
        api: Some("https://crates.io".to_string()),
    };

    let url = config.download_url(name, version).ok_or(anyhow!(
        "unable to download baseline for {}-{}",
        name,
        version
    ))?;

    let parent_dir = root.join("releaser").join("target");
    std::fs::create_dir_all(&parent_dir)?;
    let extract_path = PathBuf::from(&parent_dir).join(format!("{name}-{version}"));

    if extract_path.exists() {
        return Ok(extract_path);
    }

    let response = reqwest::blocking::get(url)?;
    let bytes = response.bytes()?;

    let decoder = GzDecoder::new(&bytes[..]);
    let mut archive = Archive::new(decoder);
    archive.unpack(&parent_dir)?;

    Ok(extract_path)
}

fn read_features(crate_path: &Path) -> Result<HashSet<String>, anyhow::Error> {
    let cargo_toml_path = crate_path.join("Cargo.toml");

    if !cargo_toml_path.exists() {
        return Err(anyhow!("Cargo.toml not found at {:?}", cargo_toml_path));
    }

    let manifest = cargo_manifest::Manifest::from_path(&cargo_toml_path)?;

    let mut set = HashSet::new();
    if let Some(features) = manifest.features {
        for f in features.keys() {
            set.insert(f.clone());
        }
    }
    if let Some(deps) = manifest.dependencies {
        for (k, v) in deps.iter() {
            if v.optional() {
                set.insert(k.clone());
            }
        }
    }

    Ok(set)
}

fn build_doc_json(krate: &Crate, config: &BuildConfig) -> Result<PathBuf, anyhow::Error> {
    let target_dir = std::env::var("CARGO_TARGET_DIR");

    let target_path = if let Ok(target) = target_dir {
        PathBuf::from(target)
    } else {
        PathBuf::from(&krate.path).join("target")
    };

    let current_path = target_path;
    let current_path = if let Some(target) = &config.target {
        current_path.join(target.clone())
    } else {
        current_path
    };
    let current_path = current_path
        .join("doc")
        .join(format!("{}.json", krate.name.to_string().replace("-", "_")));

    std::fs::remove_file(&current_path).ok();
    let features = config.features.clone();

    log::info!(
        "Building doc json for {} with features: {:?}",
        krate.name,
        features
    );

    let envs = vec![(
        "RUSTDOCFLAGS",
        "--cfg docsrs --cfg not_really_docsrs --cfg semver_checks",
    )];

    // always use `specific nightly` toolchain so we don't have to deal with potentially
    // different versions of the doc-json
    let mut cargo_args = vec![
        "+nightly-2025-06-29".to_string(),
        "rustdoc".to_string(),
        "--lib".to_string(),
        "--output-format=json".to_string(),
        "-Zunstable-options".to_string(),
        "-Zhost-config".to_string(),
        "-Ztarget-applies-to-host".to_string(),
        "-Zbuild-std=alloc,core".to_string(),
    ];
    if let Some(target) = &config.target {
        cargo_args.push(format!("--target={}", target));
    }
    if !features.is_empty() {
        cargo_args.push(format!("--features={}", features.join(",")));
    }
    cargo_args
        .push("--config=host.rustflags=[\"--cfg=instability_disable_unstable_docs\"]".to_string());
    log::debug!("{cargo_args:#?}");
    crate::cargo::run_with_env(&cargo_args, &krate.path, envs, false)?;
    Ok(current_path)
}
