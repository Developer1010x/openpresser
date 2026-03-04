/// OpenPresser Middle-Out (PPMO) compression engine.
///
/// Public API:
/// ```
/// use engine::{compress, decompress, CompressOptions};
///
/// let opts = CompressOptions::default();
/// let compressed = compress(b"hello world", &opts).unwrap();
/// let original   = decompress(&compressed).unwrap();
/// assert_eq!(original, b"hello world");
/// ```

pub mod block;
pub mod compress;
pub mod decompress;
pub mod format;
pub mod huffman;
pub mod lz;
pub mod middle_out;

pub use compress::{compress, CompressOptions};
pub use decompress::decompress;

use thiserror::Error;

#[derive(Debug, Error)]
pub enum EngineError {
    #[error("invalid header: {0}")]
    InvalidHeader(String),

    #[error("CRC32 checksum mismatch")]
    ChecksumMismatch,

    #[error("decompression error: {0}")]
    DecompressError(String),

    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
}

#[cfg(test)]
mod integration_tests {
    use super::*;

    fn round_trip(data: &[u8], parallel: bool) {
        let opts = CompressOptions {
            parallel,
            block_size: 65536,
            max_depth: 32,
        };
        let compressed = compress(data, &opts).expect("compress failed");
        let recovered = decompress(&compressed).expect("decompress failed");
        assert_eq!(
            recovered, data,
            "round-trip failed for {} bytes (parallel={})",
            data.len(), parallel
        );
    }

    #[test]
    fn empty() {
        round_trip(b"", false);
        round_trip(b"", true);
    }

    #[test]
    fn zeros() {
        round_trip(&vec![0u8; 100_000], false);
        round_trip(&vec![0u8; 100_000], true);
    }

    #[test]
    fn random_data() {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};
        let mut data = Vec::with_capacity(50_000);
        for i in 0u64..50_000 {
            let mut h = DefaultHasher::new();
            i.hash(&mut h);
            data.push(h.finish() as u8);
        }
        round_trip(&data, false);
        round_trip(&data, true);
    }

    #[test]
    fn text_data() {
        let text = b"The quick brown fox jumps over the lazy dog. \
                     OpenPresser compression is the best. \
                     Middle-out algorithm beats all others. "
            .repeat(1000);
        round_trip(&text, false);
        round_trip(&text, true);
    }

    #[test]
    fn single_byte() {
        round_trip(b"X", false);
    }

    #[test]
    fn parallel_equals_sequential() {
        let data: Vec<u8> = (0..200_000u64)
            .map(|i| (i.wrapping_mul(6364136223846793005) >> 56) as u8)
            .collect();

        let seq_opts = CompressOptions { parallel: false, block_size: 65536, max_depth: 32 };
        let par_opts = CompressOptions { parallel: true,  block_size: 65536, max_depth: 32 };

        let seq_compressed = compress(&data, &seq_opts).unwrap();
        let par_compressed = compress(&data, &par_opts).unwrap();

        // Decompressed output must be identical even if compressed bytes differ
        let seq_out = decompress(&seq_compressed).unwrap();
        let par_out = decompress(&par_compressed).unwrap();
        assert_eq!(seq_out, par_out);
        assert_eq!(seq_out, data);
    }

    #[test]
    fn multi_block() {
        let data = vec![0xABu8; 200_000];
        let opts = CompressOptions { parallel: false, block_size: 65536, max_depth: 32 };
        let compressed = compress(&data, &opts).unwrap();
        let recovered = decompress(&compressed).unwrap();
        assert_eq!(recovered, data);
    }

    #[test]
    fn dickens_file_round_trip() {
        // Read the dickens test corpus if available
        if let Ok(data) = std::fs::read("testdata/dickens.txt") {
            round_trip(&data, false);
            round_trip(&data, true);
        }
    }

    #[test]
    fn dickens_file_round_trip_via_file() {
        // Mimics CLI: compress, write to file, read file, decompress
        if let Ok(data) = std::fs::read("testdata/dickens.txt") {
            let opts = CompressOptions {
                parallel: true,
                block_size: 65536,
                max_depth: 32,
            };
            let compressed = compress(&data, &opts).expect("compress failed");

            // Write to file and read back (like CLI does)
            let tmp_path = "/tmp/dickens_test_roundtrip.ppmo";
            std::fs::write(tmp_path, &compressed).expect("write failed");
            let read_back = std::fs::read(tmp_path).expect("read failed");

            assert_eq!(compressed, read_back, "file I/O corrupted data");

            let recovered = decompress(&read_back).expect("decompress of file data failed");
            assert_eq!(recovered, data, "round-trip via file failed");
        }
    }

    #[test]
    fn decompress_cli_compressed_file() {
        // Compress dickens.txt with default options, then decompress and verify round-trip
        let original = std::fs::read("/home/hacker69i/Desktop/project/pied-piper/testdata/dickens.txt")
            .expect("Cannot read testdata/dickens.txt");
        let opts = CompressOptions::default();
        let compressed = compress(&original, &opts).expect("compression failed");
        eprintln!("Compressed dickens.txt: {} -> {} bytes", original.len(), compressed.len());
        let recovered = decompress(&compressed).expect("decompress failed");
        assert_eq!(recovered.len(), original.len(), "length mismatch");
        assert_eq!(recovered, original, "data mismatch");
    }
}
