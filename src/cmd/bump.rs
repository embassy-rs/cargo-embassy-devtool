use crate::bump::bump;
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

    bump(ctx, name, newver)?;

    Ok(())
}
