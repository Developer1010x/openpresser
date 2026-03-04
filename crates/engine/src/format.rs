/// PPMO file format constants and header I/O.
///
/// Layout:
///   0    4   Magic b"PPMO"
///   4    1   Version 0x01
///   5    1   Flags  (bit0 = parallel)
///   6    4   u32 LE block count
///  10    8   u64 LE original size
///  18    4   u32 LE block size
///  22    4   u32 LE CRC32 of header bytes 0..22
///  26   N×12 Block directory: (compressed_len u32, uncompressed_len u32, crc32 u32)
///   —    —   Block data (concatenated)

use crate::EngineError;

pub const MAGIC: &[u8; 4] = b"PPMO";
pub const VERSION: u8 = 0x01;
pub const FLAG_PARALLEL: u8 = 0x01;
pub const HEADER_BASE_LEN: usize = 26; // bytes 0..26 (before block directory)

/// Compute CRC32 (ISO 3309 / IEEE 802.3) of `data`.
pub fn crc32(data: &[u8]) -> u32 {
    const TABLE: [u32; 256] = make_crc_table();
    let mut crc: u32 = 0xFFFF_FFFF;
    for &b in data {
        crc = TABLE[((crc ^ b as u32) & 0xFF) as usize] ^ (crc >> 8);
    }
    crc ^ 0xFFFF_FFFF
}

const fn make_crc_table() -> [u32; 256] {
    let mut table = [0u32; 256];
    let mut i = 0usize;
    while i < 256 {
        let mut c = i as u32;
        let mut k = 0usize;
        while k < 8 {
            if c & 1 != 0 {
                c = 0xEDB8_8320 ^ (c >> 1);
            } else {
                c >>= 1;
            }
            k += 1;
        }
        table[i] = c;
        i += 1;
    }
    table
}

#[derive(Debug, Clone)]
pub struct PpmoHeader {
    pub flags: u8,
    pub block_count: u32,
    pub original_size: u64,
    pub block_size: u32,
}

#[derive(Debug, Clone)]
pub struct BlockEntry {
    pub compressed_len: u32,
    pub uncompressed_len: u32,
    pub crc32: u32,
}

/// Serialize the full file header (base + block directory) to a `Vec<u8>`.
pub fn write_header(hdr: &PpmoHeader, directory: &[BlockEntry]) -> Vec<u8> {
    let mut buf = Vec::with_capacity(HEADER_BASE_LEN + directory.len() * 12);

    // bytes 0..22 (before CRC field)
    buf.extend_from_slice(MAGIC);          // 0
    buf.push(VERSION);                     // 4
    buf.push(hdr.flags);                   // 5
    buf.extend_from_slice(&hdr.block_count.to_le_bytes());  // 6
    buf.extend_from_slice(&hdr.original_size.to_le_bytes()); // 10
    buf.extend_from_slice(&hdr.block_size.to_le_bytes());    // 18

    // CRC32 over bytes 0..22
    let header_crc = crc32(&buf[..22]);
    buf.extend_from_slice(&header_crc.to_le_bytes()); // 22

    // Block directory
    for entry in directory {
        buf.extend_from_slice(&entry.compressed_len.to_le_bytes());
        buf.extend_from_slice(&entry.uncompressed_len.to_le_bytes());
        buf.extend_from_slice(&entry.crc32.to_le_bytes());
    }

    buf
}

/// Parse the file header from `data`, return `(PpmoHeader, Vec<BlockEntry>, data_offset)`.
pub fn read_header(data: &[u8]) -> Result<(PpmoHeader, Vec<BlockEntry>, usize), EngineError> {
    if data.len() < HEADER_BASE_LEN {
        return Err(EngineError::InvalidHeader("file too short".into()));
    }

    // Magic
    if &data[0..4] != MAGIC {
        return Err(EngineError::InvalidHeader("bad magic".into()));
    }

    // Version
    if data[4] != VERSION {
        return Err(EngineError::InvalidHeader(format!(
            "unsupported version 0x{:02X}",
            data[4]
        )));
    }

    let flags = data[5];
    let block_count = u32::from_le_bytes(data[6..10].try_into().unwrap());
    let original_size = u64::from_le_bytes(data[10..18].try_into().unwrap());
    let block_size = u32::from_le_bytes(data[18..22].try_into().unwrap());

    // Verify header CRC
    let stored_crc = u32::from_le_bytes(data[22..26].try_into().unwrap());
    let computed_crc = crc32(&data[0..22]);
    if stored_crc != computed_crc {
        return Err(EngineError::ChecksumMismatch);
    }

    let dir_offset = HEADER_BASE_LEN;
    let dir_end = dir_offset + block_count as usize * 12;
    if data.len() < dir_end {
        return Err(EngineError::InvalidHeader("truncated block directory".into()));
    }

    let mut directory = Vec::with_capacity(block_count as usize);
    for i in 0..block_count as usize {
        let base = dir_offset + i * 12;
        let compressed_len = u32::from_le_bytes(data[base..base + 4].try_into().unwrap());
        let uncompressed_len =
            u32::from_le_bytes(data[base + 4..base + 8].try_into().unwrap());
        let block_crc = u32::from_le_bytes(data[base + 8..base + 12].try_into().unwrap());
        directory.push(BlockEntry {
            compressed_len,
            uncompressed_len,
            crc32: block_crc,
        });
    }

    Ok((
        PpmoHeader {
            flags,
            block_count,
            original_size,
            block_size,
        },
        directory,
        dir_end,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn header_round_trip() {
        let hdr = PpmoHeader {
            flags: FLAG_PARALLEL,
            block_count: 3,
            original_size: 196_608,
            block_size: 65536,
        };
        let dir = vec![
            BlockEntry { compressed_len: 1234, uncompressed_len: 65536, crc32: 0xDEAD_BEEF },
            BlockEntry { compressed_len: 999,  uncompressed_len: 65536, crc32: 0xCAFE_BABE },
            BlockEntry { compressed_len: 50,   uncompressed_len: 1024,  crc32: 0x1234_5678 },
        ];

        let bytes = write_header(&hdr, &dir);
        let (hdr2, dir2, offset) = read_header(&bytes).unwrap();

        assert_eq!(hdr2.flags, hdr.flags);
        assert_eq!(hdr2.block_count, hdr.block_count);
        assert_eq!(hdr2.original_size, hdr.original_size);
        assert_eq!(hdr2.block_size, hdr.block_size);
        assert_eq!(dir2.len(), dir.len());
        for (a, b) in dir.iter().zip(dir2.iter()) {
            assert_eq!(a.compressed_len, b.compressed_len);
            assert_eq!(a.uncompressed_len, b.uncompressed_len);
            assert_eq!(a.crc32, b.crc32);
        }
        assert_eq!(offset, HEADER_BASE_LEN + 3 * 12);
    }

    #[test]
    fn crc32_known_vector() {
        // CRC32 of b"123456789" == 0xCBF43926
        assert_eq!(crc32(b"123456789"), 0xCBF43926);
    }

    #[test]
    fn header_bad_magic() {
        let mut bytes = write_header(
            &PpmoHeader { flags: 0, block_count: 0, original_size: 0, block_size: 65536 },
            &[],
        );
        bytes[0] = b'X';
        assert!(matches!(read_header(&bytes), Err(EngineError::InvalidHeader(_))));
    }

    #[test]
    fn header_bad_crc() {
        let mut bytes = write_header(
            &PpmoHeader { flags: 0, block_count: 0, original_size: 0, block_size: 65536 },
            &[],
        );
        bytes[22] ^= 0xFF; // corrupt CRC
        assert!(matches!(read_header(&bytes), Err(EngineError::ChecksumMismatch)));
    }
}
