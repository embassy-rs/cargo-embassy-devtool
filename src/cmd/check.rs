use crate::{
    cmd::build::{Args, BuildCommand, run_build_command},
    types::Context,
};
use anyhow::Result;

pub fn run(ctx: &Context, args: Args) -> Result<()> {
    run_build_command(ctx, args, BuildCommand::Check)
}
