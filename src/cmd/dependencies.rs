use crate::types::Context;
use anyhow::Result;

#[derive(Debug, clap::Args)]
/// List all dependencies for a crate
pub struct Args {
    /// Crate name to print dependencies for.
    #[arg(value_name = "CRATE")]
    pub crate_name: String,
}

pub fn run(ctx: &Context, args: Args) -> Result<()> {
    if let Some(krate) = ctx.crates.get(&args.crate_name) {
        println!("+ {}-{}", args.crate_name, krate.version);

        let deps = ctx.recursive_dependencies(std::iter::once(args.crate_name.as_str()));
        for dep_name in deps {
            if dep_name != args.crate_name {
                if let Some(dep_crate) = ctx.crates.get(&dep_name) {
                    println!("|- {}-{}", dep_name, dep_crate.version);
                }
            }
        }
    } else {
        eprintln!("Crate '{}' not found", args.crate_name);
    }
    Ok(())
}
