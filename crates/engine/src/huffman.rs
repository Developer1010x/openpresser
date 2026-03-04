/// Canonical Huffman coding (RFC 1951 compatible).
///
/// Symbol space:
///   0..=255  — literal bytes
///   256      — end-of-block marker
///   257..=270 — match lengths (length = symbol - 257 + MIN_MATCH, with overflow handled)
///   (distances encoded as raw 16-bit integers inline)
///
/// Actually, for simplicity MOLZ encodes tokens as follows:
///   - Literal: symbol = byte value (0..=255)
///   - Match:   symbol = 256 + length (length 4..=258 → symbols 260..=514), distance as raw 15-bit value
///   - EOB:     symbol = 515 (= 256 + MAX_MATCH + 1, safely above all match symbols)
///
/// Max code length = 15 bits (Package-Merge clamped).
/// MSB-first bitstream.

use crate::EngineError;

const MAX_CODE_LEN: usize = 15;
/// End-of-block symbol. Must be > 256 + MAX_MATCH (= 256 + 258 = 514).
const EOB: u16 = 515;

// ────────────────────────────────────────────────────────────────────────────
// Public API types
// ────────────────────────────────────────────────────────────────────────────

/// A fully built canonical Huffman table.
#[derive(Debug, Clone)]
pub struct HuffmanTable {
    /// Encode: symbol → (code: u32, len: u8)
    pub encode: Vec<(u32, u8)>,
    /// Decode: (code, len) lookup — built as a flat array indexed by code value
    ///   We use a simple canonical decode table: sorted by (len, code).
    pub lengths: Vec<(u16, u8)>, // (symbol, length) pairs, sorted for serialisation
    /// Max symbol value + 1
    pub symbol_count: usize,
    /// Decode table: array of (symbol, len) indexed by up to MAX_CODE_LEN bits
    decode_table: Vec<(u16, u8)>,
}

impl HuffmanTable {
    /// Build a Huffman table from a frequency table.
    /// `freqs[symbol]` = count; symbols with freq=0 are excluded.
    pub fn build(freqs: &[u64]) -> Self {
        let n = freqs.len();

        // Collect non-zero symbols
        let mut symbols: Vec<(u64, u16)> = freqs
            .iter()
            .enumerate()
            .filter(|(_, &f)| f > 0)
            .map(|(i, &f)| (f, i as u16))
            .collect();

        if symbols.is_empty() {
            // Edge case: no data
            return Self::empty(n);
        }

        // Always include EOB
        // (If EOB has freq 0, add it with freq 1)
        if EOB as usize >= n || freqs[EOB as usize] == 0 {
            symbols.push((1, EOB));
        }

        // If only one unique symbol, assign length 1
        let lengths_map: Vec<u8> = if symbols.len() == 1 {
            let sym = symbols[0].1 as usize;
            let mut m = vec![0u8; n.max(sym + 1)];
            m[sym] = 1;
            m
        } else {
            package_merge(&symbols, n.max(EOB as usize + 1), MAX_CODE_LEN)
        };

        Self::from_lengths(&lengths_map)
    }

    /// Build from a length table `lengths[symbol] = code_length` (0 = not present).
    pub fn from_lengths(lengths_map: &[u8]) -> Self {
        let n = lengths_map.len();
        let mut lengths: Vec<(u16, u8)> = lengths_map
            .iter()
            .enumerate()
            .filter(|(_, &l)| l > 0)
            .map(|(i, &l)| (i as u16, l))
            .collect();

        // Sort by (length, symbol) for canonical assignment
        lengths.sort_by_key(|&(s, l)| (l, s));

        // Assign canonical codes
        let mut encode = vec![(0u32, 0u8); n];
        let mut code: u32 = 0;
        let mut prev_len = 0u8;
        for &(sym, len) in &lengths {
            if prev_len > 0 {
                code = (code + 1) << (len - prev_len);
            }
            encode[sym as usize] = (code, len);
            prev_len = len;
        }

        // Build decode table (up to 2^MAX_CODE_LEN entries)
        let table_size = 1usize << MAX_CODE_LEN;
        let mut decode_table = vec![(u16::MAX, 0u8); table_size];
        for &(sym, len) in &lengths {
            let (c, _) = encode[sym as usize];
            // Fill all entries that share this prefix
            let pad = MAX_CODE_LEN - len as usize;
            let base = (c as usize) << pad;
            for i in 0..(1usize << pad) {
                decode_table[base | i] = (sym, len);
            }
        }

        HuffmanTable {
            encode,
            lengths,
            symbol_count: n,
            decode_table,
        }
    }

    fn empty(n: usize) -> Self {
        HuffmanTable {
            encode: vec![(0, 0); n],
            lengths: vec![],
            symbol_count: n,
            decode_table: vec![],
        }
    }

    pub fn encode_symbol(&self, sym: u16) -> (u32, u8) {
        self.encode[sym as usize]
    }

    /// Decode one symbol from `bits` (MSB-aligned, MAX_CODE_LEN bits available).
    /// Returns `(symbol, code_len)`.
    pub fn decode_symbol(&self, bits: u32) -> Result<(u16, u8), EngineError> {
        let idx = (bits >> (32 - MAX_CODE_LEN)) as usize;
        let (sym, len) = self.decode_table[idx];
        if sym == u16::MAX {
            return Err(EngineError::DecompressError("invalid Huffman code".into()));
        }
        Ok((sym, len))
    }
}

// ────────────────────────────────────────────────────────────────────────────
// Package-Merge length-limited Huffman
// ────────────────────────────────────────────────────────────────────────────

fn package_merge(symbols: &[(u64, u16)], alphabet: usize, max_len: usize) -> Vec<u8> {
    // Coin-collector / package-merge algorithm (Larmore & Hirschberg)
    // Returns code lengths for all symbols in `alphabet`.

    let mut sorted = symbols.to_vec();
    sorted.sort_by_key(|&(f, _)| f);

    // Start with leaves
    let mut items: Vec<(u64, Vec<u16>)> = sorted
        .iter()
        .map(|&(f, s)| (f, vec![s]))
        .collect();

    for _ in 0..max_len - 1 {
        // Package pairs
        let mut packages: Vec<(u64, Vec<u16>)> = Vec::new();
        let mut i = 0;
        while i + 1 < items.len() {
            let merged_freq = items[i].0 + items[i + 1].0;
            let mut merged_syms = items[i].1.clone();
            merged_syms.extend_from_slice(&items[i + 1].1);
            packages.push((merged_freq, merged_syms));
            i += 2;
        }
        // Merge packages with original leaves (sorted merge)
        let leaves: Vec<(u64, Vec<u16>)> = sorted
            .iter()
            .map(|&(f, s)| (f, vec![s]))
            .collect();
        items = merge_sorted(leaves, packages);
    }

    // Take first 2*(n-1) items
    let n = sorted.len();
    let take = (2 * n).saturating_sub(2).min(items.len());
    let mut counts = vec![0u8; alphabet];
    for item in items.into_iter().take(take) {
        for sym in item.1 {
            if (sym as usize) < alphabet {
                counts[sym as usize] += 1;
            }
        }
    }
    counts
}

fn merge_sorted(
    a: Vec<(u64, Vec<u16>)>,
    b: Vec<(u64, Vec<u16>)>,
) -> Vec<(u64, Vec<u16>)> {
    let mut out = Vec::with_capacity(a.len() + b.len());
    let mut ai = 0;
    let mut bi = 0;
    while ai < a.len() && bi < b.len() {
        if a[ai].0 <= b[bi].0 {
            out.push(a[ai].clone());
            ai += 1;
        } else {
            out.push(b[bi].clone());
            bi += 1;
        }
    }
    out.extend_from_slice(&a[ai..]);
    out.extend_from_slice(&b[bi..]);
    out
}

// ────────────────────────────────────────────────────────────────────────────
// BitWriter (MSB-first)
// ────────────────────────────────────────────────────────────────────────────

pub struct BitWriter {
    buf: Vec<u8>,
    acc: u64,
    bits: u32, // bits in accumulator
}

impl BitWriter {
    pub fn new() -> Self {
        BitWriter { buf: Vec::new(), acc: 0, bits: 0 }
    }

    /// Write `len` bits from `code` (MSB-first, code stored in low bits).
    pub fn write_bits(&mut self, code: u32, len: u8) {
        if len == 0 {
            return;
        }
        self.acc = (self.acc << len) | (code as u64 & ((1u64 << len) - 1));
        self.bits += len as u32;
        while self.bits >= 8 {
            self.bits -= 8;
            self.buf.push((self.acc >> self.bits) as u8);
        }
    }

    /// Flush remaining bits (zero-padded to byte boundary).
    pub fn finish(mut self) -> Vec<u8> {
        if self.bits > 0 {
            self.buf.push((self.acc << (8 - self.bits)) as u8);
        }
        self.buf
    }
}

// ────────────────────────────────────────────────────────────────────────────
// BitReader (MSB-first)
// ────────────────────────────────────────────────────────────────────────────

pub struct BitReader<'a> {
    data: &'a [u8],
    pos: usize,   // byte position
    acc: u64,     // bit accumulator (MSB = next bit to read)
    bits: u32,    // bits available in acc
}

impl<'a> BitReader<'a> {
    pub fn new(data: &'a [u8]) -> Self {
        BitReader { data, pos: 0, acc: 0, bits: 0 }
    }

    fn refill(&mut self) {
        while self.bits <= 56 && self.pos < self.data.len() {
            self.acc = (self.acc << 8) | self.data[self.pos] as u64;
            self.pos += 1;
            self.bits += 8;
        }
    }

    /// Peek at the top `MAX_CODE_LEN` bits (MSB-aligned u32) without consuming.
    pub fn peek32(&mut self) -> u32 {
        self.refill();
        if self.bits >= 32 {
            (self.acc >> (self.bits - 32)) as u32
        } else {
            (self.acc << (32 - self.bits)) as u32
        }
    }

    /// Consume `n` bits.
    pub fn consume(&mut self, n: u8) {
        self.bits = self.bits.saturating_sub(n as u32);
        // Mask off consumed bits
        if self.bits < 64 {
            self.acc &= (1u64 << self.bits) - 1;
        }
    }

    /// Read exactly `n` bits as a u32.
    pub fn read_bits(&mut self, n: u8) -> Result<u32, EngineError> {
        self.refill();
        if self.bits < n as u32 {
            return Err(EngineError::DecompressError("unexpected end of bitstream".into()));
        }
        self.bits -= n as u32;
        let val = (self.acc >> self.bits) as u32;
        self.acc &= (1u64 << self.bits) - 1;
        Ok(val & ((1u32 << n) - 1))
    }
}

// ────────────────────────────────────────────────────────────────────────────
// Serialise / deserialise the Huffman table
// ────────────────────────────────────────────────────────────────────────────

/// Serialise `table` as: u16 LE count, then (u16 LE symbol, u8 len) × count.
pub fn serialise_table(table: &HuffmanTable) -> Vec<u8> {
    let mut out = Vec::with_capacity(2 + table.lengths.len() * 3);
    let count = table.lengths.len() as u16;
    out.extend_from_slice(&count.to_le_bytes());
    for &(sym, len) in &table.lengths {
        out.extend_from_slice(&sym.to_le_bytes());
        out.push(len);
    }
    out
}

/// Deserialise a Huffman table from `data[offset..]`.
/// Returns `(table, bytes_consumed)`.
pub fn deserialise_table(data: &[u8]) -> Result<(HuffmanTable, usize), EngineError> {
    if data.len() < 2 {
        return Err(EngineError::DecompressError("table too short".into()));
    }
    let count = u16::from_le_bytes(data[0..2].try_into().unwrap()) as usize;
    let needed = 2 + count * 3;
    if data.len() < needed {
        return Err(EngineError::DecompressError("truncated table".into()));
    }
    let max_sym = if count == 0 {
        0usize
    } else {
        let mut m = 0usize;
        for i in 0..count {
            let base = 2 + i * 3;
            let sym = u16::from_le_bytes(data[base..base + 2].try_into().unwrap()) as usize;
            if sym > m {
                m = sym;
            }
        }
        m + 1
    };

    let mut lengths_map = vec![0u8; max_sym];
    for i in 0..count {
        let base = 2 + i * 3;
        let sym = u16::from_le_bytes(data[base..base + 2].try_into().unwrap()) as usize;
        let len = data[base + 2];
        if sym < lengths_map.len() {
            lengths_map[sym] = len;
        }
    }

    Ok((HuffmanTable::from_lengths(&lengths_map), needed))
}

// ────────────────────────────────────────────────────────────────────────────
// Encode / decode token streams
// ────────────────────────────────────────────────────────────────────────────

use crate::lz::Token;

/// Encode tokens → (HuffmanTable, bitstream bytes).
pub fn encode_tokens(tokens: &[Token]) -> (HuffmanTable, Vec<u8>) {
    // Build frequency table.
    // Symbol space: literals 0..=255, match symbols 256+length (260..=514 for len 4..=258),
    // EOB = 515.  Freqs array size = EOB+1 = 516.
    let mut freqs = vec![0u64; EOB as usize + 1];
    for tok in tokens {
        match tok {
            Token::Literal(b) => freqs[*b as usize] += 1,
            Token::Match { length, .. } => {
                let sym = 256usize + *length as usize;
                // sym is at most 256+258=514 < EOB(515), safe to index directly
                freqs[sym] += 1;
            }
        }
    }
    freqs[EOB as usize] += 1; // EOB always present

    let table = HuffmanTable::build(&freqs);
    let mut bw = BitWriter::new();

    for tok in tokens {
        match tok {
            Token::Literal(b) => {
                let (code, len) = table.encode_symbol(*b as u16);
                bw.write_bits(code, len);
            }
            Token::Match { distance, length } => {
                let sym = (256 + *length as usize) as u16;
                let (code, len) = table.encode_symbol(sym);
                bw.write_bits(code, len);
                // Distance: raw 16-bit (MAX_DIST = 32768 needs 16 bits)
                bw.write_bits(*distance as u32, 16);
            }
        }
    }
    // EOB
    let (code, len) = table.encode_symbol(EOB);
    bw.write_bits(code, len);

    (table, bw.finish())
}

/// Decode bitstream → tokens using `table`.
pub fn decode_tokens(
    table: &HuffmanTable,
    bitstream: &[u8],
    token_capacity: usize,
) -> Result<Vec<Token>, EngineError> {
    let mut br = BitReader::new(bitstream);
    let mut tokens = Vec::with_capacity(token_capacity);

    loop {
        let bits32 = br.peek32();
        let (sym, len) = table.decode_symbol(bits32)?;
        br.consume(len);

        if sym == EOB {
            break;
        } else if sym < 256 {
            tokens.push(Token::Literal(sym as u8));
        } else {
            // Match: sym = 256 + length
            let length = (sym - 256) as u16;
            let distance = br.read_bits(16)? as u16;
            tokens.push(Token::Match { distance, length });
        }
    }

    Ok(tokens)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lz::Token;

    #[test]
    fn canonical_codes_deterministic() {
        let freqs = vec![10u64, 5, 20, 1, 7, 0, 3];
        let t1 = HuffmanTable::build(&freqs);
        let t2 = HuffmanTable::build(&freqs);
        assert_eq!(t1.lengths, t2.lengths);
    }

    #[test]
    fn encode_decode_round_trip_literals() {
        let tokens: Vec<Token> = b"hello world".iter().map(|&b| Token::Literal(b)).collect();
        let (table, bits) = encode_tokens(&tokens);
        let decoded = decode_tokens(&table, &bits, tokens.len()).unwrap();
        assert_eq!(decoded, tokens);
    }

    #[test]
    fn encode_decode_round_trip_mixed() {
        let tokens = vec![
            Token::Literal(b'a'),
            Token::Literal(b'b'),
            Token::Match { distance: 2, length: 5 },
            Token::Literal(b'c'),
            Token::Match { distance: 1, length: 10 },
        ];
        let (table, bits) = encode_tokens(&tokens);
        let decoded = decode_tokens(&table, &bits, tokens.len()).unwrap();
        assert_eq!(decoded, tokens);
    }

    #[test]
    fn table_serialise_round_trip() {
        let freqs: Vec<u64> = (0..513).map(|i| if i % 3 == 0 { 10 } else { 1 }).collect();
        let table = HuffmanTable::build(&freqs);
        let bytes = serialise_table(&table);
        let (table2, _) = deserialise_table(&bytes).unwrap();
        assert_eq!(table.lengths, table2.lengths);
    }

    #[test]
    fn single_symbol_table() {
        let mut freqs = vec![0u64; 513];
        freqs[42] = 100;
        let table = HuffmanTable::build(&freqs);
        // Symbol 42 and EOB should have length 1
        assert!(table.lengths.iter().any(|&(s, l)| s == 42 && l == 1));
    }
}
