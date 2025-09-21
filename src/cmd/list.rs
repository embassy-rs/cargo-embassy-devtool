use crate::types::Context;
use anyhow::Result;
use petgraph::visit::Bfs;

#[derive(Debug, clap::Args)]
/// All crates and their direct dependencies
pub struct Args;

pub fn run(ctx: &Context, _args: Args) -> Result<()> {
    let ordered = petgraph::algo::toposort(&ctx.graph.g, None).unwrap();
    for node in ordered.iter() {
        let start = ctx.graph.g.node_weight(*node).unwrap();
        let mut bfs = Bfs::new(&ctx.graph.g, *node);
        while let Some(node) = bfs.next(&ctx.graph.g) {
            let weight = ctx.graph.g.node_weight(node).unwrap();
            let c = ctx.crates.get(weight).unwrap();
            if weight == start {
                println!("+ {}-{}", weight, c.version);
            } else {
                println!("|- {}-{}", weight, c.version);
            }
        }
        println!();
    }
    Ok(())
}
