use crate::types::Context;
use anyhow::Result;

/// List all dependencies for a crate
#[derive(Debug, clap::Args)]
pub struct Args {
    /// Crate name to print dependencies for.
    #[arg(value_name = "CRATE")]
    pub crate_name: String,
}

pub fn run(ctx: &Context, args: Args) -> Result<()> {
    if let Some(krate) = ctx.crates.get(&args.crate_name) {
        println!("+ {}-{}", args.crate_name, krate.version);

        let dependents = ctx.recursive_dependents(std::iter::once(args.crate_name.as_str()));
        for dependent_name in dependents {
            if dependent_name != args.crate_name {
                if let Some(dependent_crate) = ctx.crates.get(&dependent_name) {
                    println!("|- {}-{}", dependent_name, dependent_crate.version);
                }
            }
        }
    } else {
        eprintln!("Crate '{}' not found", args.crate_name);
    }
    Ok(())
}
