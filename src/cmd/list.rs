use crate::types::Context;
use anyhow::Result;

#[derive(Debug, clap::Args)]
/// All crates and their direct dependencies
pub struct Args;

pub fn run(ctx: &Context, _args: Args) -> Result<()> {
    let ordered = ctx.topological_sort();
    for crate_name in ordered {
        if let Some(krate) = ctx.crates.get(&crate_name) {
            println!("+ {}-{}", crate_name, krate.version);

            let deps = ctx.recursive_dependencies(std::iter::once(crate_name.as_str()));
            for dep_name in deps {
                if dep_name != crate_name {
                    if let Some(dep_crate) = ctx.crates.get(&dep_name) {
                        println!("|- {}-{}", dep_name, dep_crate.version);
                    }
                }
            }
            println!();
        }
    }
    Ok(())
}
