use std::{fs, process::Command};

use crate::types::{Context, ParsedRustfmt, ParsedToolchain};
use anyhow::{Result, anyhow};
use indicatif::{ProgressBar, ProgressStyle};
use rayon::prelude::*;
use walkdir::{DirEntry, WalkDir};

#[derive(Debug, clap::Args)]
/// Format all files in a folder. Defaults to cwd.
pub struct Args {
    /// The dir to format
    #[clap(long, default_value = ".")]
    pub root: String,
}

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

pub fn run(_ctx: &Context, args: Args) -> Result<()> {
    // if we're not in the root, exit
    if !fs::exists(".git").unwrap_or_default() {
        return Err(anyhow!("not in root of repository"));
    }

    let parsed: ParsedRustfmt = toml::from_slice(&checkout_file("rustfmt.toml")?)?;
    let edition = parsed.edition;

    // try to check out the rust toolchain nightly file, and rename it to rust-toolchain.toml
    let Some(toolchain_file) = ["rust-toolchain-nightly.toml", "rust-toolchain.toml"]
        .iter()
        .filter_map(|file| checkout_file(file).ok())
        .find(|file| str::from_utf8(file).is_ok_and(|file| !file.trim().is_empty()))
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

    let rustfmt = str::from_utf8(&output.stdout)?.trim();

    let paths: Vec<_> = WalkDir::new(args.root)
        .into_iter()
        .filter_entry(not_target)
        .filter_map(Result::ok)
        .filter(is_file)
        .map(|e| e.into_path())
        .collect();

    let style = ProgressStyle::with_template("\x1b[96m{msg}\x1b[0m [{bar:30}] {pos}/{len}")
        .unwrap()
        .progress_chars("=> ");

    let bar = ProgressBar::new(paths.len() as u64)
        .with_style(style)
        .with_message("Formatting");

    paths
        .par_chunks(80)
        .map(|chunk| {
            let output = Command::new(rustfmt)
                .arg("--unstable-features")
                .arg("--edition")
                .arg(&edition)
                .args(chunk)
                .output()?;

            bar.inc(chunk.len() as u64);

            if !output.status.success() {
                Err(anyhow!(
                    "{}",
                    String::from_utf8_lossy(&output.stderr.to_vec())
                ))
            } else {
                Ok(())
            }
        })
        .collect::<anyhow::Result<()>>()?;

    bar.finish();

    Ok(())
}
