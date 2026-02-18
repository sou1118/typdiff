use std::fs;

use anyhow::{Context, Result};
use clap::Parser;

#[derive(Parser)]
#[command(
    name = "typdiff",
    about = "Generate a diff markup for two Typst documents"
)]
struct Cli {
    /// Path to the old Typst file
    old: String,

    /// Path to the new Typst file
    new: String,

    /// Output file path (defaults to stdout)
    #[arg(short, long)]
    output: Option<String>,
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    let old_source =
        fs::read_to_string(&cli.old).with_context(|| format!("failed to read {}", cli.old))?;
    let new_source =
        fs::read_to_string(&cli.new).with_context(|| format!("failed to read {}", cli.new))?;

    let old_blocks: Vec<_> = typdiff::parse::parse(&old_source)
        .into_iter()
        .filter(|b| !matches!(b, typdiff::Block::Parbreak))
        .collect();
    let new_blocks: Vec<_> = typdiff::parse::parse(&new_source)
        .into_iter()
        .filter(|b| !matches!(b, typdiff::Block::Parbreak))
        .collect();

    let diff_results = typdiff::diff::diff(&old_blocks, &new_blocks);

    let output = typdiff::render::render(&diff_results);

    match cli.output {
        Some(path) => {
            fs::write(&path, &output).with_context(|| format!("failed to write {}", path))?;
        }
        None => {
            print!("{}", output);
        }
    }

    Ok(())
}
