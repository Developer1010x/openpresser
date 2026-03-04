/// openpresser — OpenPresser compression CLI
///
/// Commands:
///   openpresser compress   <input> <output> [--block-kb 64] [--depth 32] [--no-parallel]
///   openpresser decompress <input> <output>
///   openpresser bench      <input> [--iters 3]
///   openpresser score      <input>

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use engine::{compress, decompress, CompressOptions};
use indicatif::{ProgressBar, ProgressStyle};
use std::path::PathBuf;
use std::time::Instant;
use weissman::{compute_score, display_score};

// ── CLI definition ────────────────────────────────────────────────────────

#[derive(Parser)]
#[command(name = "openpresser", about = "OpenPresser PPMO compression engine", version)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Compress a file using PPMO
    Compress {
        /// Input file path
        input: PathBuf,
        /// Output file path
        output: PathBuf,
        /// Block size in KB
        #[arg(long = "block-kb", default_value_t = 64)]
        block_kb: usize,
        /// Hash chain search depth
        #[arg(long = "depth", default_value_t = 32)]
        depth: usize,
        /// Disable parallel block compression
        #[arg(long = "no-parallel")]
        no_parallel: bool,
    },

    /// Decompress a PPMO file
    Decompress {
        /// Input PPMO file
        input: PathBuf,
        /// Output file path
        output: PathBuf,
    },

    /// Benchmark compression speed and ratio
    Bench {
        /// Input file to benchmark
        input: PathBuf,
        /// Number of iterations to average
        #[arg(long = "iters", default_value_t = 3)]
        iters: usize,
    },

    /// Compute and display Weissman score
    Score {
        /// Input file to score
        input: PathBuf,
    },
}

// ── Main ─────────────────────────────────────────────────────────────────

fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Commands::Compress { input, output, block_kb, depth, no_parallel } => {
            cmd_compress(&input, &output, block_kb, depth, !no_parallel)
        }
        Commands::Decompress { input, output } => cmd_decompress(&input, &output),
        Commands::Bench { input, iters } => cmd_bench(&input, iters),
        Commands::Score { input } => cmd_score(&input),
    }
}

// ── Compress ─────────────────────────────────────────────────────────────

fn cmd_compress(
    input: &PathBuf,
    output: &PathBuf,
    block_kb: usize,
    depth: usize,
    parallel: bool,
) -> Result<()> {
    let data = std::fs::read(input)
        .with_context(|| format!("reading {}", input.display()))?;

    let pb = progress_bar(data.len() as u64, "Compressing");

    let opts = CompressOptions {
        block_size: block_kb * 1024,
        max_depth: depth,
        parallel,
    };

    let start = Instant::now();
    let compressed = compress(&data, &opts)
        .map_err(|e| anyhow::anyhow!("compression failed: {}", e))?;
    let elapsed = start.elapsed();

    pb.finish_and_clear();

    std::fs::write(output, &compressed)
        .with_context(|| format!("writing {}", output.display()))?;

    let ratio = data.len() as f64 / compressed.len() as f64;
    let speed = (data.len() as f64 / 1_048_576.0) / elapsed.as_secs_f64();

    println!(
        "Compressed {} → {} bytes  ({:.4}×)  in {:.2?}  [{:.2} MB/s]",
        data.len(),
        compressed.len(),
        ratio,
        elapsed,
        speed
    );

    Ok(())
}

// ── Decompress ───────────────────────────────────────────────────────────

fn cmd_decompress(input: &PathBuf, output: &PathBuf) -> Result<()> {
    let data = std::fs::read(input)
        .with_context(|| format!("reading {}", input.display()))?;

    let pb = progress_bar(data.len() as u64, "Decompressing");

    let start = Instant::now();
    let original = decompress(&data)
        .map_err(|e| anyhow::anyhow!("decompression failed: {}", e))?;
    let elapsed = start.elapsed();

    pb.finish_and_clear();

    std::fs::write(output, &original)
        .with_context(|| format!("writing {}", output.display()))?;

    println!(
        "Decompressed {} → {} bytes  in {:.2?}",
        data.len(),
        original.len(),
        elapsed
    );

    Ok(())
}

// ── Bench ────────────────────────────────────────────────────────────────

fn cmd_bench(input: &PathBuf, iters: usize) -> Result<()> {
    let data = std::fs::read(input)
        .with_context(|| format!("reading {}", input.display()))?;

    println!("Benchmarking {} ({} bytes, {} iterations)…", input.display(), data.len(), iters);

    let ws = compute_score(&data, iters).context("benchmarking")?;
    display_score(&ws);

    Ok(())
}

// ── Score ────────────────────────────────────────────────────────────────

fn cmd_score(input: &PathBuf) -> Result<()> {
    let data = std::fs::read(input)
        .with_context(|| format!("reading {}", input.display()))?;

    println!("Scoring {} ({} bytes)…", input.display(), data.len());

    let ws = compute_score(&data, 1).context("scoring")?;
    display_score(&ws);

    Ok(())
}

// ── Helpers ───────────────────────────────────────────────────────────────

fn progress_bar(len: u64, msg: &'static str) -> ProgressBar {
    let pb = ProgressBar::new(len);
    pb.set_style(
        ProgressStyle::with_template(
            "{msg} [{elapsed_precise}] [{bar:40.cyan/blue}] {bytes}/{total_bytes} ({eta})",
        )
        .unwrap()
        .progress_chars("=>-"),
    );
    pb.set_message(msg);
    pb
}
