/// Block splitting and parallel compression.
///
/// Per-block wire format:
///   [u32 LE pivot]
///   [u32 LE left_token_count]
///   [u16 LE Huffman table entry count]
///   [(symbol u16 LE, len u8) × count]   — Huffman table
///   [bitstream]                          — combined (left ++ right) Huffman-coded tokens

use crate::{
    compress::CompressOptions,
    format::{crc32, BlockEntry},
    huffman::{decode_tokens, deserialise_table, encode_tokens, serialise_table},
    middle_out::{decode_bidirectional, encode_bidirectional},
    EngineError,
};

/// Compress a single raw block into the PPMO per-block wire format.
pub fn compress_block(block: &[u8], opts: &CompressOptions) -> Vec<u8> {
    let (pivot, left_tokens, right_tokens) = encode_bidirectional(block, opts.max_depth);
    let left_count = left_tokens.len() as u32;

    // Combine: left tokens first, then right tokens
    let mut combined = left_tokens;
    combined.extend(right_tokens);

    let (table, bitstream) = encode_tokens(&combined);
    let table_bytes = serialise_table(&table);

    let mut out = Vec::with_capacity(8 + table_bytes.len() + bitstream.len());
    out.extend_from_slice(&(pivot as u32).to_le_bytes());       // 4 bytes
    out.extend_from_slice(&left_count.to_le_bytes());           // 4 bytes
    out.extend_from_slice(&table_bytes);                        // 2 + count*3 bytes
    out.extend_from_slice(&bitstream);
    out
}

/// Decompress a single compressed block back to raw bytes.
pub fn decompress_block(
    compressed: &[u8],
    uncompressed_len: usize,
) -> Result<Vec<u8>, EngineError> {
    if compressed.len() < 10 {
        return Err(EngineError::DecompressError("block too short".into()));
    }
    let pivot = u32::from_le_bytes(compressed[0..4].try_into().unwrap()) as usize;
    let left_count = u32::from_le_bytes(compressed[4..8].try_into().unwrap()) as usize;

    let (table, table_consumed) = deserialise_table(&compressed[8..])?;
    let bitstream_start = 8 + table_consumed;
    let bitstream = &compressed[bitstream_start..];

    let combined = decode_tokens(&table, bitstream, uncompressed_len)?;

    if left_count > combined.len() {
        return Err(EngineError::DecompressError(format!(
            "left_count {} > total tokens {}",
            left_count,
            combined.len()
        )));
    }

    let left_tokens = combined[..left_count].to_vec();
    let right_tokens = combined[left_count..].to_vec();

    Ok(decode_bidirectional(pivot, &left_tokens, &right_tokens, uncompressed_len))
}

/// Split `data` into blocks of at most `block_size` bytes.
pub fn split_into_blocks(data: &[u8], block_size: usize) -> Vec<&[u8]> {
    if data.is_empty() {
        return vec![];
    }
    data.chunks(block_size).collect()
}

/// Compress all blocks, optionally in parallel.
/// Returns `(compressed_blocks, block_entries)`.
pub fn compress_blocks(
    blocks: &[&[u8]],
    opts: &CompressOptions,
) -> (Vec<Vec<u8>>, Vec<BlockEntry>) {
    let compressed: Vec<Vec<u8>> = if opts.parallel && blocks.len() > 1 {
        use rayon::prelude::*;
        blocks.par_iter().map(|b| compress_block(b, opts)).collect()
    } else {
        blocks.iter().map(|b| compress_block(b, opts)).collect()
    };

    let entries: Vec<BlockEntry> = compressed
        .iter()
        .zip(blocks.iter())
        .map(|(c, &b)| BlockEntry {
            compressed_len: c.len() as u32,
            uncompressed_len: b.len() as u32,
            crc32: crc32(b),
        })
        .collect();

    (compressed, entries)
}
