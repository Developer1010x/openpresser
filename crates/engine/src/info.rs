/// Inspection and integrity verification for PPMO archives.
///
/// These helpers let callers examine a compressed buffer's metadata and check
/// its integrity **without** keeping the fully decompressed output around — the
/// header, the block directory and every per-block CRC32 are validated in a
/// single streaming pass.
///
/// ```
/// use engine::{compress, inspect, verify, CompressOptions};
///
/// let compressed = compress(b"hello openpresser", &CompressOptions::default()).unwrap();
///
/// let meta = inspect(&compressed).unwrap();
/// assert_eq!(meta.original_size, 17);
///
/// assert!(verify(&compressed).is_ok());
/// ```

use crate::{
    block::decompress_block,
    format::{crc32, read_header},
    EngineError,
};

/// High-level metadata describing a PPMO archive, derived purely from its
/// header and block directory (no block payloads are decoded).
#[derive(Debug, Clone)]
pub struct ArchiveInfo {
    /// PPMO format version byte.
    pub version: u8,
    /// `true` if the archive was produced with parallel block compression.
    pub parallel: bool,
    /// Number of blocks in the archive.
    pub block_count: u32,
    /// Original (uncompressed) size in bytes.
    pub original_size: u64,
    /// Configured block size in bytes.
    pub block_size: u32,
    /// Total compressed size of the archive in bytes (header + payload).
    pub compressed_size: u64,
    /// Compression ratio (`original_size / compressed_size`); `1.0` for an
    /// empty archive.
    pub ratio: f64,
    /// Space saved as a fraction in `0.0..=1.0` (`1 - compressed/original`);
    /// `0.0` for an empty archive.
    pub space_saving: f64,
}

/// `version` byte is fixed by the parser, but exposing it keeps callers honest
/// if the format ever grows additional versions.
const PPMO_VERSION: u8 = crate::format::VERSION;

/// Read an archive's metadata without decompressing any block payloads.
///
/// This only parses and CRC-checks the header/block directory, so it is cheap
/// even for very large archives. Use [`verify`] for a full integrity check.
pub fn inspect(input: &[u8]) -> Result<ArchiveInfo, EngineError> {
    let (header, _directory, _data_offset) = read_header(input)?;

    let compressed_size = input.len() as u64;
    let original_size = header.original_size;

    let (ratio, space_saving) = if original_size == 0 || compressed_size == 0 {
        (1.0, 0.0)
    } else {
        let ratio = original_size as f64 / compressed_size as f64;
        let saving = 1.0 - (compressed_size as f64 / original_size as f64);
        (ratio, saving)
    };

    Ok(ArchiveInfo {
        version: PPMO_VERSION,
        parallel: header.flags & crate::format::FLAG_PARALLEL != 0,
        block_count: header.block_count,
        original_size,
        block_size: header.block_size,
        compressed_size,
        ratio,
        space_saving,
    })
}

/// Fully verify the integrity of a PPMO archive.
///
/// Every block is decompressed and its CRC32 is compared against the value
/// stored in the block directory, and the reconstructed total length is checked
/// against the header's `original_size`. The decompressed bytes are discarded
/// as the pass proceeds, so peak memory stays bounded by a single block rather
/// than the whole output.
///
/// Returns the validated [`ArchiveInfo`] on success, or the first
/// [`EngineError`] encountered.
pub fn verify(input: &[u8]) -> Result<ArchiveInfo, EngineError> {
    let (header, directory, mut data_offset) = read_header(input)?;

    let mut total: u64 = 0;
    for (i, entry) in directory.iter().enumerate() {
        let end = data_offset + entry.compressed_len as usize;
        if end > input.len() {
            return Err(EngineError::DecompressError(format!(
                "block {i}: truncated block data"
            )));
        }
        let block_data = &input[data_offset..end];

        let raw = decompress_block(block_data, entry.uncompressed_len as usize)?;
        if crc32(&raw) != entry.crc32 {
            return Err(EngineError::ChecksumMismatch);
        }

        total += raw.len() as u64;
        data_offset = end;
        // `raw` is dropped here, keeping peak memory to a single block.
    }

    if total != header.original_size {
        return Err(EngineError::DecompressError(format!(
            "size mismatch: header claims {} bytes, blocks sum to {}",
            header.original_size, total
        )));
    }

    inspect(input)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{compress, CompressOptions};

    fn opts() -> CompressOptions {
        CompressOptions { parallel: false, block_size: 65536, max_depth: 32 }
    }

    #[test]
    fn inspect_reports_sizes() {
        let data = vec![0xABu8; 200_000];
        let compressed = compress(&data, &opts()).unwrap();
        let meta = inspect(&compressed).unwrap();

        assert_eq!(meta.original_size, 200_000);
        assert_eq!(meta.compressed_size, compressed.len() as u64);
        assert_eq!(meta.block_size, 65536);
        assert_eq!(meta.block_count, 4); // 200_000 / 65_536 -> 4 blocks
        assert!(meta.ratio > 1.0);
        assert!(meta.space_saving > 0.0 && meta.space_saving < 1.0);
        assert_eq!(meta.version, PPMO_VERSION);
    }

    #[test]
    fn inspect_empty_archive() {
        let compressed = compress(b"", &opts()).unwrap();
        let meta = inspect(&compressed).unwrap();
        assert_eq!(meta.original_size, 0);
        assert_eq!(meta.block_count, 0);
        assert_eq!(meta.ratio, 1.0);
        assert_eq!(meta.space_saving, 0.0);
    }

    #[test]
    fn verify_accepts_valid_archive() {
        let data = b"OpenPresser integrity check works end to end.".repeat(500);
        let compressed = compress(&data, &opts()).unwrap();
        let meta = verify(&compressed).unwrap();
        assert_eq!(meta.original_size, data.len() as u64);
    }

    #[test]
    fn verify_detects_corruption() {
        let data = b"corrupt me please".repeat(2000);
        let mut compressed = compress(&data, &opts()).unwrap();
        // Flip a byte deep inside the block payload, past the header/directory.
        let last = compressed.len() - 1;
        compressed[last] ^= 0xFF;
        // Either a CRC mismatch or a decode error is acceptable; it must not
        // silently succeed.
        assert!(verify(&compressed).is_err());
    }

    #[test]
    fn verify_detects_truncation() {
        let data = vec![7u8; 100_000];
        let compressed = compress(&data, &opts()).unwrap();
        let truncated = &compressed[..compressed.len() - 5];
        assert!(verify(truncated).is_err());
    }
}
