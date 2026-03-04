/// Top-level decompress() entry point.

use crate::{
    block::decompress_block,
    format::{crc32, read_header},
    EngineError,
};

/// Decompress a PPMO-compressed buffer and return the original bytes.
pub fn decompress(input: &[u8]) -> Result<Vec<u8>, EngineError> {
    let (header, directory, mut data_offset) = read_header(input)?;

    eprintln!("[decompress] input_len={}, block_count={}, original_size={}, block_size={}, data_offset={}",
        input.len(), header.block_count, header.original_size, header.block_size, data_offset);

    let mut out = Vec::with_capacity(header.original_size as usize);

    for (i, entry) in directory.iter().enumerate() {
        let end = data_offset + entry.compressed_len as usize;
        if end > input.len() {
            return Err(EngineError::DecompressError("truncated block data".into()));
        }
        let block_data = &input[data_offset..end];

        eprintln!("[decompress] block {}: data_offset={}, compressed_len={}, uncompressed_len={}, stored_crc=0x{:08x}",
            i, data_offset, entry.compressed_len, entry.uncompressed_len, entry.crc32);

        let raw = decompress_block(block_data, entry.uncompressed_len as usize)?;

        // Verify block CRC32
        let block_crc = crc32(&raw);
        if block_crc != entry.crc32 {
            eprintln!("[decompress] block {}: CRC MISMATCH! raw_len={}, stored=0x{:08x}, computed=0x{:08x}",
                i, raw.len(), entry.crc32, block_crc);
            // Print first few bytes
            eprintln!("[decompress] block {} first 32 bytes: {:?}", i, &raw[..raw.len().min(32)]);
            return Err(EngineError::ChecksumMismatch);
        }

        eprintln!("[decompress] block {}: OK (raw_len={})", i, raw.len());
        out.extend_from_slice(&raw);
        data_offset = end;
    }

    if out.len() as u64 != header.original_size {
        return Err(EngineError::DecompressError(format!(
            "size mismatch: expected {} bytes, got {}",
            header.original_size,
            out.len()
        )));
    }

    Ok(out)
}
