use crate::{update_changelog, update_graph_deps, update_version};
use crate::types::Context;
use anyhow::Result;

#[derive(Debug, clap::Args)]
/// Force set a dependency to a version.
///
/// Can be used to override result of prepare release
pub struct Args {
    /// Crate name to print dependencies for.
    #[arg(value_name = "CRATE")]
    pub crate_name: String,

    #[arg(value_name = "CRATE_VERSION")]
    pub crate_version: String,
}

pub fn run(ctx: &mut Context, args: Args) -> Result<()> {
    let newver = &args.crate_version;
    let name = &args.crate_name;
    let c = ctx.crates.get_mut(name).unwrap();
    let oldver = c.version.clone();
    update_version(c, newver)?;

    let c = ctx.crates.get(name).unwrap();
    // Update all nodes further down the tree
    update_graph_deps(ctx, &ctx.graph, name, &oldver, newver)?;
    update_graph_deps(ctx, &ctx.build_graph, name, &oldver, newver)?;
    update_graph_deps(ctx, &ctx.dev_graph, name, &oldver, newver)?;

    // Update changelog
    update_changelog(&ctx.root, c)?;
    Ok(())
}
