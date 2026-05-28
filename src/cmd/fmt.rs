use std::{fs, process::Command};

use crate::types::Context;
use anyhow::{Result, anyhow};
use itertools::Itertools;
use walkdir::{DirEntry, WalkDir};

#[derive(Debug, clap::Args)]
/// All crates and their direct dependencies
pub struct Args;

fn checkout_file(file: &str) -> Result<()> {
    Command::new("git")
        .arg("checkout")
        .arg("--")
        .arg(file)
        .output()?;

    if !fs::exists(file).unwrap_or_default() {
        Err(anyhow!("file does not exist"))
    } else {
        Ok(())
    }
}

fn is_file(entry: &DirEntry) -> bool {
    entry.file_type().is_file() && entry.path().extension().is_some_and(|ext| ext == "rs")
}

fn not_target(entry: &DirEntry) -> bool {
    entry.file_name().to_string_lossy() != "target"
}

pub fn run(_ctx: &Context, _args: Args) -> Result<()> {
    // if we're not in the root, exit
    if !fs::exists(".git").unwrap_or_default() {
        return Err(anyhow!("not in root of repository"));
    }

    // save the contents of rust-toolchain.toml and rust-toolchain-nightly.toml, if they exist
    let rust_toolchain = fs::read("rust-toolchain.toml").ok();
    let rust_toolchain_nightly = fs::read("rust-toolchain-nightly.toml").ok();

    // try to check out the rust toolchain nightly file, and rename it to rust-toolchain.toml
    let Some(toolchain_file) = ["rust-toolchain-nightly.toml", "rust-toolchain.toml"]
        .iter()
        .filter_map(|file| checkout_file(file).map(|_| file).ok())
        .next()
    else {
        return Err(anyhow!("no toolchain file could be checked out"));
    };

    fs::write("rust-toolchain.toml", fs::read(toolchain_file)?)?;

    let chunk_size = 80;
    for chunk in &WalkDir::new(".")
        .into_iter()
        .filter_entry(not_target)
        .filter_map(Result::ok)
        .filter(is_file)
        .map(|e| e.into_path())
        .chunks(chunk_size)
    {
        let output = Command::new("rustfmt")
            .arg("--unstable-features")
            .arg("--edition")
            .arg("2024")
            .args(chunk)
            .output()?;

        if !output.status.success() {
            return Err(anyhow!(
                "{}",
                String::from_utf8_lossy(&output.stderr.to_vec())
            ));
        }
    }

    if let Some(rust_toolchain) = rust_toolchain {
        fs::write("rust-toolchain.toml", rust_toolchain)?;
    }

    if let Some(rust_toolchain) = rust_toolchain_nightly {
        fs::write("rust-toolchain-nightly.toml", rust_toolchain)?;
    }

    Ok(())
}
