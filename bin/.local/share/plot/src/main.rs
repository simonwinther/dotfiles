mod gui;
use anyhow::{Context, Result, ensure};
use clap::Parser;
use formula_plot::{
    model::{Plot, Viewport},
    render,
};
use std::path::PathBuf;

#[derive(Parser)]
#[command(
    name = "plot",
    bin_name = "plot",
    version,
    about = "Plot mathematical formulas in a native graph window.",
    after_help = "Examples:\n  plot 'sin(x), cos(x)'\n  plot 'y = 2x + 1' --range -5 5\n  plot 'clip(x,-1,1), x^2' --output graph.svg\n\nDrag to pan, scroll to zoom, Home to reset, Ctrl+S to save."
)]
struct Cli {
    /// One or more comma-separated formulas using x
    #[arg(required=true,num_args=1..)]
    formula: Vec<String>,
    /// Horizontal range (vertical range starts at -20 to 20)
    #[arg(short='r',long,num_args=2,default_values_t=[-10.0,10.0],allow_hyphen_values=true,value_names=["MIN","MAX"])]
    range: Vec<f64>,
    /// Save PNG, SVG, or PDF without opening a window or needing a display
    #[arg(short, long, value_name = "FILE")]
    output: Option<PathBuf>,
    /// Canvas/export width in pixels
    #[arg(long, default_value_t = 1000)]
    width: u32,
    /// Canvas/export height in pixels
    #[arg(long, default_value_t = 600)]
    height: u32,
}

fn run(cli: Cli) -> Result<()> {
    ensure!(
        (400..=8192).contains(&cli.width)
            && (300..=8192).contains(&cli.height)
            && u64::from(cli.width) * u64::from(cli.height) <= 32_000_000,
        "Image dimensions must be 400–8192 × 300–8192, with at most 32 million pixels"
    );
    let view = Viewport::new(cli.range[0], cli.range[1])?;
    let formula = cli.formula.join(" ");
    let plot = Plot::parse(&formula)?;
    for warning in &plot.warnings {
        eprintln!("plot: {warning}");
    }
    if let Some(path) = cli.output {
        render::export(&plot, view, &path, cli.width, cli.height)?;
        println!("Saved {}", path.display());
    } else {
        gui::run(plot, view, formula, cli.width, cli.height).context(
            "Open the plot window; use --output FILE.png, FILE.svg, or FILE.pdf for a display-free export",
        )?;
    }
    Ok(())
}
fn main() {
    if let Err(error) = run(Cli::parse()) {
        eprintln!("plot: {error:#}");
        std::process::exit(1);
    }
}
