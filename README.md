# OpenPresser

OpenPresser is a from-scratch **lossless compression toolkit** written in Rust,
built around a "middle-out" bidirectional codec (PPMO — *Presser Middle-Out*).
It ships a reusable compression library, a command-line tool, and a benchmark
harness that scores PPMO against `gzip` and `zstd` using a Weissman-style
metric.

## How it works

The PPMO pipeline compresses each block independently so blocks can be encoded
in parallel with [`rayon`](https://crates.io/crates/rayon):

1. **Block splitting** — the input is chunked into fixed-size blocks
   (default 64 KiB).
2. **Middle-out encoding** — each block is encoded *bidirectionally* from a
   pivot point outward, producing a stream of left tokens and right tokens.
3. **LZ matching** — repeated sequences are replaced with back-references found
   via a bounded hash-chain search (`--depth`).
4. **Huffman coding** — the combined token stream is entropy-coded with a
   canonical Huffman table that is serialized into the block.
5. **Framing & checksums** — a `PPMO` header records the block directory, and a
   CRC32 is stored for the header and for every block so corruption is caught on
   read.

### File format

```
0    4   Magic b"PPMO"
4    1   Version (0x01)
5    1   Flags  (bit0 = parallel)
6    4   u32 LE block count
10   8   u64 LE original size
18   4   u32 LE block size
22   4   u32 LE CRC32 of header bytes 0..22
26  N×12 Block directory: (compressed_len u32, uncompressed_len u32, crc32 u32)
 —    —  Block payloads (concatenated)
```

## Project layout

```
crates/
  engine/    PPMO codec: blocks, middle-out, LZ, Huffman, framing, inspect/verify
  weissman/  Weissman-score benchmark vs. gzip + zstd
  cli/       `openpresser` command-line front-end
testdata/    Sample corpora (dickens.txt, random.bin)
```

## Building

```sh
cargo build --release
```

The CLI binary is produced at `target/release/openpresser`.

## CLI usage

```sh
# Compress (tune block size, search depth, parallelism)
openpresser compress  input.txt  output.ppmo [--block-kb 64] [--depth 32] [--no-parallel]

# Decompress
openpresser decompress output.ppmo restored.txt

# Inspect archive metadata WITHOUT decompressing the payload
openpresser info       output.ppmo

# Verify integrity: decompress every block and check its CRC32
openpresser verify     output.ppmo

# Benchmark PPMO vs gzip vs zstd and print a Weissman score
openpresser bench      input.txt [--iters 3]
openpresser score      input.txt
```

### `info` / `verify`

`openpresser info` parses only the header and block directory, so it reports the
block count, configured block size, original/compressed sizes, compression ratio
and percentage of space saved without paying the cost of a full decompression —
handy for large archives.

`openpresser verify` performs a complete integrity pass: it decompresses every
block, compares each block's CRC32 against the stored value, and confirms the
reconstructed length matches the header. Decompressed bytes are discarded as the
pass proceeds, so peak memory stays bounded by a single block rather than the
whole output.

## Library usage

```rust
use engine::{compress, decompress, inspect, verify, CompressOptions};

let opts = CompressOptions::default();
let compressed = compress(b"hello openpresser", &opts).unwrap();

// Round-trip
let original = decompress(&compressed).unwrap();
assert_eq!(original, b"hello openpresser");

// Cheap metadata (header only)
let meta = inspect(&compressed).unwrap();
println!("{} bytes -> {} bytes", meta.original_size, meta.compressed_size);

// Full integrity check (returns the validated metadata)
verify(&compressed).unwrap();
```

### `CompressOptions`

| Field        | Default | Meaning                                        |
|--------------|---------|------------------------------------------------|
| `block_size` | `65536` | Block size in bytes.                           |
| `max_depth`  | `32`    | Hash-chain search depth for LZ matching.       |
| `parallel`   | `true`  | Compress blocks in parallel via `rayon`.       |

## Testing & benchmarks

```sh
cargo test                       # unit + round-trip integration tests
cargo bench -p engine            # criterion micro-benchmarks
```

## License

See repository for license details.
