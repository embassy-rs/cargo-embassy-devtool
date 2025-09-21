use crate::types::Context;
use anyhow::Result;
use std::path::PathBuf;
use std::process::Command;

#[derive(Debug, clap::Args)]
/// Build documentation using docserver for publishable crates
pub struct Args {
    /// Build docs for only these crates (must be publishable)
    #[clap(long = "crate")]
    pub crates: Vec<String>,
    
    /// Output directory for generated documentation
    #[clap(short, long)]
    pub output: PathBuf,
}

pub fn run(ctx: &Context, args: Args) -> Result<()> {
    let crates_to_build = if args.crates.is_empty() {
        // Build docs for all publishable crates
        ctx.crates
            .iter()
            .filter(|(_, crate_info)| crate_info.publish)
            .map(|(crate_id, _)| crate_id.clone())
            .collect()
    } else {
        // Build docs for specific crates (only if they are publishable)
        let mut crates = Vec::new();
        for crate_name in &args.crates {
            if !ctx.crates.contains_key(crate_name) {
                anyhow::bail!("Crate '{}' not found", crate_name);
            }
            let crate_info = &ctx.crates[crate_name];
            if !crate_info.publish {
                println!("⚠️  Skipping non-publishable crate: {}", crate_name);
                continue;
            }
            crates.push(crate_name.clone());
        }
        crates
    };

    if crates_to_build.is_empty() {
        println!("No publishable crates found to build documentation for.");
        return Ok(());
    }

    let mut success_count = 0;
    let mut failed_crates = Vec::new();
    let mut is_first_invocation = true;

    for crate_id in &crates_to_build {
        let crate_info = &ctx.crates[crate_id];
        let input_path = &crate_info.path;
        let output_path = args.output.join("crates").join(crate_id).join("git.zup");
        
        println!("Building docs for crate: {}", crate_id);
        
        let mut cmd = Command::new("docserver");
        cmd.arg("build")
            .arg("-i")
            .arg(input_path)
            .arg("-o")
            .arg(&output_path);
        
        // Add --output-static for the first docserver invocation
        if is_first_invocation {
            let static_output = args.output.join("static");
            cmd.arg("--output-static").arg(&static_output);
            is_first_invocation = false;
        }
        
        match cmd.status() {
            Ok(status) if status.success() => {
                println!("✅ Successfully built docs for {}", crate_id);
                success_count += 1;
            }
            Ok(status) => {
                eprintln!("❌ Failed to build docs for {} (exit code: {:?})", crate_id, status.code());
                failed_crates.push(crate_id.clone());
            }
            Err(e) => {
                eprintln!("❌ Failed to run docserver for {}: {}", crate_id, e);
                failed_crates.push(crate_id.clone());
            }
        }
    }

    println!("\nSummary:");
    println!("✅ Successfully built docs for {} crates", success_count);
    
    if !failed_crates.is_empty() {
        println!("❌ Failed to build docs for {} crates:", failed_crates.len());
        for crate_name in &failed_crates {
            println!("  - {}", crate_name);
        }
        anyhow::bail!("Failed to build docs for {} crates", failed_crates.len());
    }

    Ok(())
}