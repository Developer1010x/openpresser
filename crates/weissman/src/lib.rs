/// Weissman score computation.
///
/// Formula:
///   W = α × (r / r_gzip) × (ln(s / s_gzip) + 1.0)
///
/// where:
///   r = PPMO compression ratio (original_size / compressed_size)
///   s = PPMO throughput in MB/s
///   r_gzip, s_gzip = same metrics for gzip
///   α = 1.0

use anyhow::{Context, Result};
use engine::{compress, CompressOptions};
use flate2::write::GzEncoder;
use flate2::Compression;
use std::io::Write;
use std::time::{Duration, Instant};

const ALPHA: f64 = 1.0;

#[derive(Debug, Clone)]
pub struct BenchResult {
    pub name: &'static str,
    pub original_bytes: usize,
    pub compressed_bytes: usize,
    pub ratio: f64,
    pub speed_mbs: f64,
}

impl BenchResult {
    pub fn ratio_str(&self) -> String {
        format!("{:.4}×", self.ratio)
    }
    pub fn speed_str(&self) -> String {
        format!("{:.2} MB/s", self.speed_mbs)
    }
}

#[derive(Debug, Clone)]
pub struct WeissmanScore {
    pub ppmo: BenchResult,
    pub gzip: BenchResult,
    pub zstd: BenchResult,
    pub score: f64,
}

/// Run PPMO, gzip, and zstd on `data` (averaging over `iters` runs) and compute
/// the Weissman score.
pub fn compute_score(data: &[u8], iters: usize) -> Result<WeissmanScore> {
    let ppmo = bench_ppmo(data, iters).context("PPMO bench")?;
    let gzip = bench_gzip(data, iters).context("gzip bench")?;
    let zstd = bench_zstd(data, iters).context("zstd bench")?;

    let score = weissman(ppmo.ratio, ppmo.speed_mbs, gzip.ratio, gzip.speed_mbs);

    Ok(WeissmanScore { ppmo, gzip, zstd, score })
}

fn weissman(r: f64, s: f64, r_gzip: f64, s_gzip: f64) -> f64 {
    if r_gzip == 0.0 || s_gzip == 0.0 {
        return 0.0;
    }
    ALPHA * (r / r_gzip) * ((s / s_gzip).ln() + 1.0)
}

// ── PPMO ────────────────────────────────────────────────────────────────────

fn bench_ppmo(data: &[u8], iters: usize) -> Result<BenchResult> {
    let opts = CompressOptions::default();
    // Warm-up
    let compressed = compress(data, &opts).map_err(|e| anyhow::anyhow!("{}", e))?;
    let elapsed = time_average(iters, || {
        compress(data, &opts).unwrap();
    });
    let ratio = if compressed.is_empty() {
        1.0
    } else {
        data.len() as f64 / compressed.len() as f64
    };
    Ok(BenchResult {
        name: "PPMO",
        original_bytes: data.len(),
        compressed_bytes: compressed.len(),
        ratio,
        speed_mbs: throughput_mbs(data.len(), elapsed),
    })
}

// ── gzip ────────────────────────────────────────────────────────────────────

fn bench_gzip(data: &[u8], iters: usize) -> Result<BenchResult> {
    let compressed = gzip_compress(data)?;
    let elapsed = time_average(iters, || {
        gzip_compress(data).unwrap();
    });
    let ratio = if compressed.is_empty() {
        1.0
    } else {
        data.len() as f64 / compressed.len() as f64
    };
    Ok(BenchResult {
        name: "gzip",
        original_bytes: data.len(),
        compressed_bytes: compressed.len(),
        ratio,
        speed_mbs: throughput_mbs(data.len(), elapsed),
    })
}

fn gzip_compress(data: &[u8]) -> Result<Vec<u8>> {
    let mut enc = GzEncoder::new(Vec::new(), Compression::default());
    enc.write_all(data)?;
    Ok(enc.finish()?)
}

// ── zstd ────────────────────────────────────────────────────────────────────

fn bench_zstd(data: &[u8], iters: usize) -> Result<BenchResult> {
    let compressed = zstd::encode_all(data, 3)?;
    let elapsed = time_average(iters, || {
        zstd::encode_all(data, 3).unwrap();
    });
    let ratio = if compressed.is_empty() {
        1.0
    } else {
        data.len() as f64 / compressed.len() as f64
    };
    Ok(BenchResult {
        name: "zstd",
        original_bytes: data.len(),
        compressed_bytes: compressed.len(),
        ratio,
        speed_mbs: throughput_mbs(data.len(), elapsed),
    })
}

// ── Utilities ───────────────────────────────────────────────────────────────

fn time_average(iters: usize, mut f: impl FnMut()) -> Duration {
    let n = iters.max(1);
    let start = Instant::now();
    for _ in 0..n {
        f();
    }
    start.elapsed() / n as u32
}

fn throughput_mbs(bytes: usize, elapsed: Duration) -> f64 {
    if elapsed.is_zero() {
        return f64::INFINITY;
    }
    (bytes as f64 / 1_048_576.0) / elapsed.as_secs_f64()
}

/// Format and display the Weissman score as a box.
pub fn display_score(ws: &WeissmanScore) {
    let w = 54usize;
    let line = "─".repeat(w);
    println!("┌{}┐", line);
    println!("│{:^54}│", "  WEISSMAN SCORE  ");
    println!("├{}┤", line);

    let header = format!("{:<10} {:>10} {:>10} {:>12}", "Engine", "Ratio", "Speed", "Compressed");
    println!("│  {:<52}│", header);
    println!("├{}┤", line);

    for res in [&ws.ppmo, &ws.gzip, &ws.zstd] {
        let row = format!(
            "{:<10} {:>10} {:>10} {:>12}",
            res.name,
            res.ratio_str(),
            res.speed_str(),
            format!("{} B", res.compressed_bytes),
        );
        println!("│  {:<52}│", row);
    }

    println!("├{}┤", line);
    println!("│{:^54}│", format!("  W = {:.6}  ", ws.score));
    println!("└{}┘", line);
}
