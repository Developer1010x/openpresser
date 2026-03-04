use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use engine::{compress, decompress, CompressOptions};

fn make_text_data(size: usize) -> Vec<u8> {
    let text = b"The quick brown fox jumps over the lazy dog. \
                 OpenPresser middle-out compression is revolutionary! \
                 Silicon City Developers change the world. ";
    let mut v = Vec::with_capacity(size);
    while v.len() < size {
        v.extend_from_slice(text);
    }
    v.truncate(size);
    v
}

fn make_random_data(size: usize) -> Vec<u8> {
    (0..size as u64)
        .map(|i| {
            let x = i.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            (x >> 56) as u8
        })
        .collect()
}

fn make_zero_data(size: usize) -> Vec<u8> {
    vec![0u8; size]
}

fn bench_compress(c: &mut Criterion) {
    let sizes = [4 * 1024, 64 * 1024, 256 * 1024];

    let mut group = c.benchmark_group("compress/text");
    for &size in &sizes {
        let data = make_text_data(size);
        group.throughput(Throughput::Bytes(size as u64));
        group.bench_with_input(BenchmarkId::from_parameter(size), &data, |b, d| {
            let opts = CompressOptions::default();
            b.iter(|| compress(d, &opts).unwrap());
        });
    }
    group.finish();

    let mut group = c.benchmark_group("compress/random");
    for &size in &sizes {
        let data = make_random_data(size);
        group.throughput(Throughput::Bytes(size as u64));
        group.bench_with_input(BenchmarkId::from_parameter(size), &data, |b, d| {
            let opts = CompressOptions::default();
            b.iter(|| compress(d, &opts).unwrap());
        });
    }
    group.finish();

    let mut group = c.benchmark_group("compress/zeros");
    for &size in &sizes {
        let data = make_zero_data(size);
        group.throughput(Throughput::Bytes(size as u64));
        group.bench_with_input(BenchmarkId::from_parameter(size), &data, |b, d| {
            let opts = CompressOptions::default();
            b.iter(|| compress(d, &opts).unwrap());
        });
    }
    group.finish();
}

fn bench_decompress(c: &mut Criterion) {
    let sizes = [64 * 1024, 256 * 1024];
    let opts = CompressOptions::default();

    let mut group = c.benchmark_group("decompress/text");
    for &size in &sizes {
        let data = make_text_data(size);
        let compressed = compress(&data, &opts).unwrap();
        group.throughput(Throughput::Bytes(size as u64));
        group.bench_with_input(
            BenchmarkId::from_parameter(size),
            &compressed,
            |b, c| {
                b.iter(|| decompress(c).unwrap());
            },
        );
    }
    group.finish();
}

criterion_group!(benches, bench_compress, bench_decompress);
criterion_main!(benches);
