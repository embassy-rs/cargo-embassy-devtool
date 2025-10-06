//! Tools for working with Cargo.

use std::ffi::OsStr;
use std::path::Path;
use std::process::{Command, Stdio};

use anyhow::{bail, Result};

use crate::windows_safe_path;

/// Execute cargo with the given arguments and from the specified directory.
pub fn run_with_env<I, K, V>(args: &[String], cwd: &Path, envs: I, capture: bool) -> Result<String>
where
    I: IntoIterator<Item = (K, V)> + core::fmt::Debug,
    K: AsRef<OsStr>,
    V: AsRef<OsStr>,
{
    if !cwd.is_dir() {
        bail!("The `cwd` argument MUST be a directory");
    }

    // Make sure to not use a UNC as CWD!
    // That would make `OUT_DIR` a UNC which will trigger things like the one fixed in https://github.com/dtolnay/rustversion/pull/51
    // While it's fixed in `rustversion` it's not fixed for other crates we are
    // using now or in future!
    let cwd = windows_safe_path(cwd);

    let args_str = args.join(" ");
    let truncated_args = if args_str.len() > 100 {
        format!("{}... ({} chars)", &args_str[..97], args_str.len())
    } else {
        args_str.clone()
    };

    println!(
        "Running `cargo {}` in {:?} - Environment {:?}",
        truncated_args, cwd, envs
    );

    let mut command = Command::new(get_cargo());

    command
        .args(args)
        .current_dir(cwd)
        .envs(envs)
        .stdout(if capture {
            Stdio::piped()
        } else {
            Stdio::inherit()
        })
        .stderr(if capture {
            Stdio::piped()
        } else {
            Stdio::inherit()
        });

    if args.iter().any(|a| a.starts_with('+')) {
        // Make sure the right cargo runs
        command.env_remove("CARGO");
    }

    let output = command.stdin(Stdio::inherit()).output()?;

    // Make sure that we return an appropriate exit code here, as Github Actions
    // requires this in order to function correctly:
    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).to_string())
    } else {
        let truncated_args = if args_str.len() > 100 {
            format!("{}... ({} chars)", &args_str[..97], args_str.len())
        } else {
            args_str
        };
        bail!(
            "Failed to execute cargo subcommand `cargo {}`",
            truncated_args,
        )
    }
}

fn get_cargo() -> String {
    // On Windows when executed via `cargo run` (e.g. via the xtask alias) the
    // `cargo` on the search path is NOT the cargo-wrapper but the `cargo` from the
    // toolchain - that one doesn't understand `+toolchain`
    #[cfg(target_os = "windows")]
    let cargo = if let Ok(cargo) = std::env::var("CARGO_HOME") {
        format!("{cargo}/bin/cargo")
    } else {
        String::from("cargo")
    };

    #[cfg(not(target_os = "windows"))]
    let cargo = String::from("cargo");

    cargo
}
