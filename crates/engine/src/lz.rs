/// LZ77-style hash chain and token types for MOLZ.
///
/// Hash function: `v.wrapping_mul(0x9E3779B9) >> 16` → 64K table.
/// Minimum match length = 4.  Maximum match length = 258.
/// Maximum match distance = 32768.

pub const MIN_MATCH: usize = 4;
pub const MAX_MATCH: usize = 258;
pub const MAX_DIST: usize = 32768;
const HASH_SIZE: usize = 65536;
const _CHAIN_LEN: usize = 8192; // max chain depth per search (reference constant)

/// A single LZ token (either a literal byte or a back-reference).
#[derive(Debug, Clone, PartialEq)]
pub enum Token {
    Literal(u8),
    Match { distance: u16, length: u16 },
}

/// Hash chain over a slice of bytes.
pub struct HashChain {
    head: Vec<u32>,   // head[hash] = most recent position
    next: Vec<u32>,   // next[pos]  = previous position with same hash (0 = none)
    max_depth: usize,
}

impl HashChain {
    pub fn new(data_len: usize, max_depth: usize) -> Self {
        HashChain {
            head: vec![u32::MAX; HASH_SIZE],
            next: vec![u32::MAX; data_len.max(1)],
            max_depth,
        }
    }

    fn hash4(data: &[u8], pos: usize) -> usize {
        if pos + 4 > data.len() {
            return 0;
        }
        let v = u32::from_le_bytes(data[pos..pos + 4].try_into().unwrap());
        ((v.wrapping_mul(0x9E37_79B9)) >> 16) as usize & (HASH_SIZE - 1)
    }

    /// Insert position `pos` into the chain.
    pub fn insert(&mut self, data: &[u8], pos: usize) {
        let h = Self::hash4(data, pos);
        let prev = self.head[h];
        self.next[pos] = prev;
        self.head[h] = pos as u32;
    }

    /// Find the longest match starting at `pos` going **forward** (pos < end).
    /// Returns `(distance, length)` if a match ≥ MIN_MATCH is found.
    pub fn find_longest_match_forward(
        &self,
        data: &[u8],
        pos: usize,
    ) -> Option<(u16, u16)> {
        if pos + MIN_MATCH > data.len() {
            return None;
        }
        let h = Self::hash4(data, pos);
        let mut best_len = MIN_MATCH - 1;
        let mut best_dist = 0u16;
        let limit = if pos > MAX_DIST { pos - MAX_DIST } else { 0 };
        let max_len = (data.len() - pos).min(MAX_MATCH);

        let mut cur = self.head[h];
        let mut depth = 0;
        while cur != u32::MAX && depth < self.max_depth {
            let cur_pos = cur as usize;
            if cur_pos < limit {
                break;
            }
            let dist = pos - cur_pos;
            if dist == 0 {
                // should not happen but guard anyway
                cur = self.next[cur_pos];
                depth += 1;
                continue;
            }
            // Count matching bytes
            let mut len = 0;
            while len < max_len && data[pos + len] == data[cur_pos + len] {
                len += 1;
            }
            if len > best_len {
                best_len = len;
                best_dist = dist as u16;
                if best_len == MAX_MATCH {
                    break;
                }
            }
            cur = self.next[cur_pos];
            depth += 1;
        }

        if best_len >= MIN_MATCH {
            Some((best_dist, best_len as u16))
        } else {
            None
        }
    }

    /// Find the longest match starting at `pos` going **backward** (pos > start).
    /// The "backward" pass works by reversing the block and using forward matching,
    /// but this function is the in-place version: it looks *ahead* in `data` to
    /// find the best run that already appears at some earlier position relative
    /// to `pos` within a reversed context.
    ///
    /// In practice for MOLZ's left pass we reverse the data first and call
    /// `find_longest_match_forward`; this function is kept for symmetry / testing.
    pub fn find_longest_match_backward(
        &self,
        data: &[u8],
        pos: usize,
    ) -> Option<(u16, u16)> {
        // Reuse forward logic — works identically since we already reversed.
        self.find_longest_match_forward(data, pos)
    }
}

/// Tokenise `data` using the hash chain.  Returns a Vec of `Token`.
/// Greedy LZ77 (no lazy matching — avoids chain corruption from early insertions).
pub fn tokenise(data: &[u8], max_depth: usize) -> Vec<Token> {
    let mut chain = HashChain::new(data.len(), max_depth);
    let mut tokens = Vec::with_capacity(data.len() / 2 + 16);
    let mut pos = 0;

    while pos < data.len() {
        chain.insert(data, pos);

        if let Some((dist, len)) = chain.find_longest_match_forward(data, pos) {
            tokens.push(Token::Match { distance: dist, length: len });
            // Insert the skipped positions into the chain for future matches
            for i in 1..len as usize {
                if pos + i < data.len() {
                    chain.insert(data, pos + i);
                }
            }
            pos += len as usize;
        } else {
            tokens.push(Token::Literal(data[pos]));
            pos += 1;
        }
    }

    tokens
}

/// Decode a token stream back into bytes.
pub fn detokenise(tokens: &[Token], capacity_hint: usize) -> Vec<u8> {
    let mut out = Vec::with_capacity(capacity_hint);
    for tok in tokens {
        match tok {
            Token::Literal(b) => out.push(*b),
            Token::Match { distance, length } => {
                let dist = *distance as usize;
                let len = *length as usize;
                if dist == 0 || dist > out.len() {
                    // Degenerate: copy from start of buffer (shouldn't normally happen)
                    let safe_dist = dist.max(1).min(out.len().max(1));
                    let start = out.len().saturating_sub(safe_dist);
                    for i in 0..len {
                        let b = if out.is_empty() { 0 } else { out[start + i % safe_dist] };
                        out.push(b);
                    }
                } else {
                    let start = out.len() - dist;
                    for i in 0..len {
                        let b = out[start + i % dist];
                        out.push(b);
                    }
                }
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn match_on_repetitive_input() {
        let data: Vec<u8> = b"abcabcabcabcabc".to_vec();
        let tokens = tokenise(&data, 32);
        // Must have at least one Match token
        assert!(tokens.iter().any(|t| matches!(t, Token::Match { .. })));
        // Round-trip
        let out = detokenise(&tokens, data.len());
        assert_eq!(out, data);
    }

    #[test]
    fn min_length_respected() {
        // A pattern of length 3 should not be encoded as a match (MIN_MATCH = 4)
        let data = b"xyzxyzxyz".to_vec();
        let tokens = tokenise(&data, 32);
        for tok in &tokens {
            if let Token::Match { length, .. } = tok {
                assert!(*length >= MIN_MATCH as u16, "match too short: {}", length);
            }
        }
    }

    #[test]
    fn round_trip_all_literals() {
        let data: Vec<u8> = (0..=255u8).collect();
        let tokens = tokenise(&data, 32);
        let out = detokenise(&tokens, data.len());
        assert_eq!(out, data);
    }

    #[test]
    fn round_trip_long_run() {
        let data = vec![0x42u8; 1024];
        let tokens = tokenise(&data, 32);
        let out = detokenise(&tokens, data.len());
        assert_eq!(out, data);
    }
}
