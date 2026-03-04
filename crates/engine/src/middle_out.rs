/// Middle-Out LZ (MOLZ) — pivot finding and bidirectional encoding.
///
/// Algorithm:
///  1. `find_pivot`: stride 64 bytes, window 512 bytes around each candidate.
///     Score = total forward-match lengths for positions in window.
///     Pick the candidate with the highest score; fallback to `B/2`.
///
///  2. `encode_bidirectional`:
///     - Right pass: tokenise block[pivot..] forward → right_tokens.
///     - Left  pass: tokenise block[..pivot] forward → left_tokens.
///       (The "middle-out" concept: pivot is the densest region; both sides
///       encoded separately so they benefit from the high-density starting
///       context at the block level.)
///
///  The left_token_count is stored in the block header so decompression
///  can cleanly split the combined token stream without fragile byte counting.

use crate::lz::{tokenise, Token};
use crate::lz::{detokenise, HashChain, MIN_MATCH};

const STRIDE: usize = 64;
const WINDOW: usize = 512;
const SCORE_DEPTH: usize = 8;

/// Find the pivot position in `block` using a density scan.
pub fn find_pivot(block: &[u8]) -> usize {
    let b = block.len();
    if b <= MIN_MATCH * 2 {
        return b / 2;
    }

    let mut best_score = 0u64;
    let mut best_pivot = b / 2;

    let mut candidate = 0usize;
    while candidate < b {
        let win_start = if candidate > WINDOW / 2 {
            candidate - WINDOW / 2
        } else {
            0
        };
        let win_end = (candidate + WINDOW / 2).min(b);
        let window = &block[win_start..win_end];

        let score = score_window(window);

        if score > best_score {
            best_score = score;
            best_pivot = candidate;
        }

        candidate += STRIDE;
    }

    // Clamp pivot so both passes have at least a few bytes
    let lo = MIN_MATCH.min(b);
    let hi = b.saturating_sub(MIN_MATCH).max(lo);
    best_pivot.clamp(lo, hi)
}

fn score_window(window: &[u8]) -> u64 {
    let mut chain = HashChain::new(window.len(), SCORE_DEPTH);
    let mut score = 0u64;
    let mut pos = 0;
    while pos < window.len() {
        chain.insert(window, pos);
        if let Some((_d, l)) = chain.find_longest_match_forward(window, pos) {
            score += l as u64;
            pos += l as usize;
        } else {
            pos += 1;
        }
    }
    score
}

/// Encode `block` bidirectionally.
/// Returns `(pivot, left_tokens, right_tokens)`.
///
/// `left_tokens`  covers `block[..pivot]` (forward LZ).
/// `right_tokens` covers `block[pivot..]` (forward LZ).
pub fn encode_bidirectional(
    block: &[u8],
    max_depth: usize,
) -> (usize, Vec<Token>, Vec<Token>) {
    let pivot = find_pivot(block);
    let left_tokens = tokenise(&block[..pivot], max_depth);
    let right_tokens = tokenise(&block[pivot..], max_depth);
    (pivot, left_tokens, right_tokens)
}

/// Decode back into the original block.
pub fn decode_bidirectional(
    pivot: usize,
    left_tokens: &[Token],
    right_tokens: &[Token],
    original_len: usize,
) -> Vec<u8> {
    let left = detokenise(left_tokens, pivot);
    let right = detokenise(right_tokens, original_len - pivot);
    let mut out = Vec::with_capacity(original_len);
    out.extend_from_slice(&left);
    out.extend_from_slice(&right);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pivot_uniform_block() {
        let block = vec![0u8; 512];
        let p = find_pivot(&block);
        let center = block.len() / 2;
        let tolerance = block.len() / 4;
        assert!(
            p >= center.saturating_sub(tolerance) && p <= center + tolerance,
            "pivot={} out of range for uniform block",
            p
        );
    }

    #[test]
    fn round_trip_small() {
        let block = b"hello world hello world!!".to_vec();
        let (pivot, left, right) = encode_bidirectional(&block, 32);
        let recovered = decode_bidirectional(pivot, &left, &right, block.len());
        assert_eq!(recovered, block);
    }

    #[test]
    fn round_trip_large() {
        let text = b"The quick brown fox jumps over the lazy dog. ";
        let mut block = Vec::with_capacity(131072);
        while block.len() < 131072 {
            block.extend_from_slice(text);
        }
        block.truncate(131072);
        let (pivot, left, right) = encode_bidirectional(&block, 32);
        let recovered = decode_bidirectional(pivot, &left, &right, block.len());
        assert_eq!(recovered, block);
    }

    #[test]
    fn round_trip_zeros() {
        let block = vec![0u8; 65536];
        let (pivot, left, right) = encode_bidirectional(&block, 32);
        let recovered = decode_bidirectional(pivot, &left, &right, block.len());
        assert_eq!(recovered, block);
    }

    #[test]
    fn round_trip_random_like() {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};
        let mut block = Vec::with_capacity(4096);
        for i in 0u64..4096 {
            let mut h = DefaultHasher::new();
            i.hash(&mut h);
            block.push(h.finish() as u8);
        }
        let (pivot, left, right) = encode_bidirectional(&block, 32);
        let recovered = decode_bidirectional(pivot, &left, &right, block.len());
        assert_eq!(recovered, block);
    }
}
