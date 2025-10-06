use crate::types::Context;
use anyhow::{anyhow, Result};
use std::collections::HashSet;
use toml_edit::{DocumentMut, Item};

#[derive(Debug, clap::Args)]
/// Check that all Cargo.toml files have correct metadata and feature configuration
pub struct Args;

pub fn run(ctx: &Context, _args: Args) -> Result<()> {
    let mut errors = Vec::new();

    for (crate_name, krate) in &ctx.crates {
        let cargo_toml_path = krate.path.join("Cargo.toml");
        let content = std::fs::read_to_string(&cargo_toml_path)
            .map_err(|e| anyhow!("Failed to read {}: {}", cargo_toml_path.display(), e))?;

        let doc: DocumentMut = content
            .parse()
            .map_err(|e| anyhow!("Failed to parse {}: {}", cargo_toml_path.display(), e))?;

        // Check package metadata
        if let Err(e) = check_package_metadata(&doc, crate_name, krate.publish) {
            errors.push(format!("{}: {}", crate_name, e));
        }

        // Check features - only for publishable crates
        if krate.publish {
            if let Err(e) = check_features(&doc) {
                errors.push(format!("{}: {}", crate_name, e));
            }
        }
    }

    if errors.is_empty() {
        println!("✅ All manifests are correct!");
        Ok(())
    } else {
        for error in &errors {
            eprintln!("❌ {}", error);
        }
        Err(anyhow!("Found {} manifest errors", errors.len()))
    }
}

fn check_package_metadata(doc: &DocumentMut, crate_name: &str, is_publishable: bool) -> Result<()> {
    let package = doc
        .get("package")
        .ok_or_else(|| anyhow!("missing [package] section"))?
        .as_table()
        .ok_or_else(|| anyhow!("[package] is not a table"))?;

    // Check edition
    let edition = package
        .get("edition")
        .ok_or_else(|| anyhow!("missing edition field"))?
        .as_str()
        .ok_or_else(|| anyhow!("edition field is not a string"))?;

    if edition != "2024" {
        return Err(anyhow!("edition should be '2024', found '{}'", edition));
    }

    // Check license
    let license = package
        .get("license")
        .ok_or_else(|| anyhow!("missing license field"))?
        .as_str()
        .ok_or_else(|| anyhow!("license field is not a string"))?;

    if license != "MIT OR Apache-2.0" {
        return Err(anyhow!(
            "license should be 'MIT OR Apache-2.0', found '{}'",
            license
        ));
    }

    // Only check repository and documentation for publishable crates
    if is_publishable {
        // Check repository
        let repository = package
            .get("repository")
            .ok_or_else(|| anyhow!("missing repository field"))?
            .as_str()
            .ok_or_else(|| anyhow!("repository field is not a string"))?;

        if repository != "https://github.com/embassy-rs/embassy" {
            return Err(anyhow!(
                "repository should be 'https://github.com/embassy-rs/embassy', found '{}'",
                repository
            ));
        }

        // Check documentation
        let documentation = package
            .get("documentation")
            .ok_or_else(|| anyhow!("missing documentation field"))?
            .as_str()
            .ok_or_else(|| anyhow!("documentation field is not a string"))?;

        let expected_documentation = format!("https://docs.embassy.dev/{}", crate_name);
        if documentation != expected_documentation {
            return Err(anyhow!(
                "documentation should be '{}', found '{}'",
                expected_documentation,
                documentation
            ));
        }
    }

    Ok(())
}

fn check_features(doc: &DocumentMut) -> Result<()> {
    // Get all optional dependencies
    let mut optional_deps: HashSet<String> = HashSet::new();

    // Check dependencies
    if let Some(deps) = doc.get("dependencies").and_then(|d| d.as_table()) {
        for (name, value) in deps.iter() {
            if is_optional_dependency(value) {
                optional_deps.insert(name.to_string());
            }
        }
    }

    // Check dev-dependencies
    if let Some(deps) = doc.get("dev-dependencies").and_then(|d| d.as_table()) {
        for (name, value) in deps.iter() {
            if is_optional_dependency(value) {
                optional_deps.insert(name.to_string());
            }
        }
    }

    // Check build-dependencies
    if let Some(deps) = doc.get("build-dependencies").and_then(|d| d.as_table()) {
        for (name, value) in deps.iter() {
            if is_optional_dependency(value) {
                optional_deps.insert(name.to_string());
            }
        }
    }

    if optional_deps.is_empty() {
        return Ok(()); // No optional dependencies to check
    }

    // Get all features that reference dependencies
    let mut referenced_deps: HashSet<String> = HashSet::new();

    if let Some(features) = doc.get("features").and_then(|f| f.as_table()) {
        for (_feature_name, feature_value) in features.iter() {
            if let Some(feature_list) = feature_value.as_array() {
                for item in feature_list.iter() {
                    if let Some(item_str) = item.as_str() {
                        if let Some(dep_name) = item_str.strip_prefix("dep:") {
                            referenced_deps.insert(dep_name.to_string());
                        }
                    }
                }
            }
        }
    }

    // Find unreferenced optional dependencies
    let unreferenced: Vec<String> = optional_deps
        .difference(&referenced_deps)
        .cloned()
        .collect();

    if !unreferenced.is_empty() {
        return Err(anyhow!(
            "optional dependencies not referenced by any feature with 'dep:': {}",
            unreferenced.join(", ")
        ));
    }

    Ok(())
}

fn is_optional_dependency(value: &Item) -> bool {
    match value {
        Item::Value(toml_edit::Value::InlineTable(table)) => table
            .get("optional")
            .and_then(|v| v.as_bool())
            .unwrap_or(false),
        _ => false,
    }
}
