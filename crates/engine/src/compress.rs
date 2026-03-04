/// Top-level compress() entry point.

use crate::{
    block::{compress_blocks, split_into_blocks},
    format::{write_header, PpmoHeader, FLAG_PARALLEL},
    EngineError,
};

/// Options controlling compression behaviour.
#[derive(Debug, Clone)]
pub struct CompressOptions {
    /// Block size in bytes (default 65536).
    pub block_size: usize,
    /// Hash chain search depth (default 32).
    pub max_depth: usize,
    /// Whether to use rayon parallel block compression (default true).
    pub parallel: bool,
}

impl Default for CompressOptions {
    fn default() -> Self {
        CompressOptions {
            block_size: 65536,
            max_depth: 32,
            parallel: true,
        }
    }
}

/// Compress `input` with PPMO and return the compressed bytes.
pub fn compress(input: &[u8], opts: &CompressOptions) -> Result<Vec<u8>, EngineError> {
    let blocks = split_into_blocks(input, opts.block_size);
    let (compressed, entries) = compress_blocks(&blocks, opts);

    let flags = if opts.parallel { FLAG_PARALLEL } else { 0 };
    let header = PpmoHeader {
        flags,
        block_count: entries.len() as u32,
        original_size: input.len() as u64,
        block_size: opts.block_size as u32,
    };

    let header_bytes = write_header(&header, &entries);

    let total: usize = compressed.iter().map(|b| b.len()).sum();
    let mut out = Vec::with_capacity(header_bytes.len() + total);
    out.extend_from_slice(&header_bytes);
    for block in &compressed {
        out.extend_from_slice(block);
    }

    Ok(out)
}
