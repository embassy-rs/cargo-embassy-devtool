use std::fs;
use std::path::Path;

use crate::types::{Context, *};
use anyhow::{anyhow, Result};
use toml_edit::{DocumentMut, Item, Value};

pub fn bump(ctx: &mut Context, name: &CrateId, new_version: &str) -> Result<(), anyhow::Error> {
    let c = ctx.crates.get_mut(name).unwrap();
    let old_version = c.version.clone();
    c.version = new_version.to_string();

    update_crate(c, new_version)?;
    for dep in &ctx.reverse_deps[name] {
        println!("Updating {name}-{old_version} -> {new_version} for {dep}");
        update_deps(&ctx.crates[dep], name, new_version)?;
    }

    let c = ctx.crates.get(name).unwrap();
    update_changelog(&ctx.root, c)?;

    Ok(())
}

fn update_crate(c: &mut Crate, new_version: &str) -> Result<()> {
    let path = c.path.join("Cargo.toml");
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

fn update_deps(to_update: &Crate, dep: &CrateId, new_version: &str) -> Result<()> {
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
