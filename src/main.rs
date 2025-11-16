use anyhow::Result;
use clap::Parser;
use reduce_recipe::reduce_recipe_file;
use std::path::PathBuf;

/// Reduce a cargo chef workspace recipe by removing unused workspace members.
///
/// This command takes a `recipe.json` produced by `cargo chef prepare --bin foo`
/// and outputs a reduced recipe containing only the workspace members that are needed.
#[derive(Parser)]
#[command(
    version = "0.1.0",
    about = "Reduces a Cargo Chef workspace recipe by filtering unused workspace members"
)]
struct Args {
    /// Path to the original recipe.json
    #[arg(
        long = "recipe-path-in",
        default_value = "recipe.json",
        value_name = "INPUT",
        help = "Path to the Cargo Chef recipe.json to reduce"
    )]
    recipe_in: PathBuf,

    /// Path to write the reduced recipe
    #[arg(
        long = "recipe-path-out",
        default_value = "recipe-reduced.json",
        value_name = "OUTPUT",
        help = "Path to write the reduced recipe.json"
    )]
    recipe_out: PathBuf,

    /// The workspace binary to keep.
    /// All of its transitive workspace dependencies will be kept.
    #[arg(
        long = "bin",
        value_name = "NAME",
        required = true,
        help = "The workspace binary to keep (all its dependent members will be kept)"
    )]
    bin: String,
}

fn main() -> Result<()> {
    let args = Args::parse();

    reduce_recipe_file(&args.recipe_in, &args.recipe_out, &args.bin)?;

    Ok(())
}
