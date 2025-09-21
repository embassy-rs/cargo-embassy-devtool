use crate::types::Context;
use anyhow::Result;
use petgraph::Direction;

/// List all dependencies for a crate
#[derive(Debug, clap::Args)]
pub struct Args {
    /// Crate name to print dependencies for.
    #[arg(value_name = "CRATE")]
    pub crate_name: String,
}

pub fn run(ctx: &Context, args: Args) -> Result<()> {
    let idx = ctx
        .graph
        .i
        .get(&args.crate_name)
        .expect("unable to find crate in tree");
    let weight = ctx.graph.g.node_weight(*idx).unwrap();
    let crt = ctx.crates.get(weight).unwrap();
    println!("+ {}-{}", weight, crt.version);
    for parent in ctx.graph.g.neighbors_directed(*idx, Direction::Incoming) {
        let weight = ctx.graph.g.node_weight(parent).unwrap();
        let crt = ctx.crates.get(weight).unwrap();
        println!("|- {}-{}", weight, crt.version);
    }
    Ok(())
}
