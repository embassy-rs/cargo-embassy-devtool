use crate::types::Context;
use anyhow::Result;
use petgraph::visit::Bfs;

#[derive(Debug, clap::Args)]
/// List all dependencies for a crate
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
    let mut bfs = Bfs::new(&ctx.graph.g, *idx);
    while let Some(node) = bfs.next(&ctx.graph.g) {
        let weight = ctx.graph.g.node_weight(node).unwrap();
        let crt = ctx.crates.get(weight).unwrap();
        if *weight == args.crate_name {
            println!("+ {}-{}", weight, crt.version);
        } else {
            println!("|- {}-{}", weight, crt.version);
        }
    }
    Ok(())
}
