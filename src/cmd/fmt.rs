use std::{fs, process::Command};

use crate::types::{Context, ParsedToolchain};
use anyhow::{Result, anyhow};
use rayon::prelude::*;
use walkdir::{DirEntry, WalkDir};

#[derive(Debug, clap::Args)]
/// All crates and their direct dependencies
pub struct Args;

fn checkout_file(file: &str) -> Result<Vec<u8>> {
    Ok(Command::new("git")
        .arg("show")
        .arg(format!("HEAD:{file}"))
        .output()?
        .stdout)
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

    // try to check out the rust toolchain nightly file, and rename it to rust-toolchain.toml
    let Some(toolchain_file) = ["rust-toolchain-nightly.toml", "rust-toolchain.toml"]
        .iter()
        .filter_map(|file| checkout_file(file).ok())
        .next()
    else {
        return Err(anyhow!("no toolchain file could be checked out"));
    };

    let parsed: ParsedToolchain = toml::from_slice(&toolchain_file)?;

    let output = Command::new("rustup")
        .arg("which")
        .arg("rustfmt")
        .arg("--toolchain")
        .arg(parsed.toolchain.channel)
        .output()?;

    if !output.status.success() {
        return Err(anyhow!(
            "{}",
            String::from_utf8_lossy(&output.stderr.to_vec())
        ));
    }

    let rustfmt = output.stdout.to_vec();
    let rustfmt = String::from_utf8_lossy(&rustfmt);
    let rustfmt = rustfmt.into_owned().trim().to_string();

    let paths: Vec<_> = WalkDir::new(".")
        .into_iter()
        .filter_entry(not_target)
        .filter_map(Result::ok)
        .filter(is_file)
        .map(|e| e.into_path())
        .collect();

    paths
        .par_chunks(80)
        .map(|chunk| {
            let output = Command::new(&rustfmt)
                .arg("--unstable-features")
                .arg("--edition")
                .arg("2024")
                .args(chunk)
                .output()?;

            if !output.status.success() {
                Err(anyhow!(
                    "{}",
                    String::from_utf8_lossy(&output.stderr.to_vec())
                ))
            } else {
                Ok(())
            }
        })
        .collect::<anyhow::Result<_>>()
}
