//! Benchmarks for bitmap operations

use criterion::{black_box, criterion_group, criterion_main, Criterion, BenchmarkId};
use pinot_core::bitmap::*;
use roaring::RoaringBitmap;

fn bench_bitmap_iteration(c: &mut Criterion) {
    let mut bitmap = RoaringBitmap::new();
    for i in (0..10000).step_by(2) {
        bitmap.insert(i);
    }

    c.bench_function("bitmap_iteration_5k", |b| {
        b.iter(|| {
            let set = BitmapDocIdSet::new(bitmap.clone(), 10000);
            let mut iter = set.iterator();
            let mut count = 0;
            while iter.next() != EOF {
                count += 1;
            }
            black_box(count)
        })
    });
}

fn bench_and_operation(c: &mut Criterion) {
    let mut bitmap1 = RoaringBitmap::new();
    let mut bitmap2 = RoaringBitmap::new();

    for i in 0..5000 {
        bitmap1.insert(i);
    }
    for i in 2500..7500 {
        bitmap2.insert(i);
    }

    c.bench_function("bitmap_and", |b| {
        b.iter(|| {
            let set1: Box<dyn DocIdSet> = Box::new(BitmapDocIdSet::new(bitmap1.clone(), 10000));
            let set2: Box<dyn DocIdSet> = Box::new(BitmapDocIdSet::new(bitmap2.clone(), 10000));
            let and_set = AndDocIdSet::new(vec![set1, set2], 10000);

            let mut iter = and_set.iterator();
            let mut count = 0;
            while iter.next() != EOF {
                count += 1;
            }
            black_box(count)
        })
    });
}

fn bench_or_operation(c: &mut Criterion) {
    let mut bitmap1 = RoaringBitmap::new();
    let mut bitmap2 = RoaringBitmap::new();

    for i in (0..5000).step_by(2) {
        bitmap1.insert(i);
    }
    for i in (1..5000).step_by(2) {
        bitmap2.insert(i);
    }

    c.bench_function("bitmap_or", |b| {
        b.iter(|| {
            let set1: Box<dyn DocIdSet> = Box::new(BitmapDocIdSet::new(bitmap1.clone(), 10000));
            let set2: Box<dyn DocIdSet> = Box::new(BitmapDocIdSet::new(bitmap2.clone(), 10000));
            let or_set = OrDocIdSet::new(vec![set1, set2], 10000);

            let mut iter = or_set.iterator();
            let mut count = 0;
            while iter.next() != EOF {
                count += 1;
            }
            black_box(count)
        })
    });
}

fn bench_not_operation(c: &mut Criterion) {
    let mut bitmap = RoaringBitmap::new();
    for i in (0..5000).step_by(2) {
        bitmap.insert(i);
    }

    c.bench_function("bitmap_not", |b| {
        b.iter(|| {
            let child: Box<dyn DocIdSet> = Box::new(BitmapDocIdSet::new(bitmap.clone(), 5000));
            let not_set = NotDocIdSet::new(child, 5000);

            let mut iter = not_set.iterator();
            let mut count = 0;
            while iter.next() != EOF {
                count += 1;
            }
            black_box(count)
        })
    });
}

fn bench_advance(c: &mut Criterion) {
    let mut bitmap = RoaringBitmap::new();
    for i in (0..100000).step_by(10) {
        bitmap.insert(i);
    }

    let targets: Vec<i32> = (0..1000).map(|i| i * 100).collect();

    c.bench_function("bitmap_advance_1k", |b| {
        b.iter(|| {
            let set = BitmapDocIdSet::new(bitmap.clone(), 100000);
            let mut iter = set.iterator();
            for &target in &targets {
                black_box(iter.advance(target));
            }
        })
    });
}

fn bench_cardinality_operations(c: &mut Criterion) {
    let mut bitmap1 = RoaringBitmap::new();
    let mut bitmap2 = RoaringBitmap::new();

    for i in 0..50000 {
        bitmap1.insert(i);
    }
    for i in 25000..75000 {
        bitmap2.insert(i);
    }

    let mut group = c.benchmark_group("cardinality");

    group.bench_function("and_cardinality", |b| {
        b.iter(|| black_box(bitmap_ops::and_cardinality(&bitmap1, &bitmap2)))
    });

    group.bench_function("or_cardinality", |b| {
        b.iter(|| black_box(bitmap_ops::or_cardinality(&bitmap1, &bitmap2)))
    });

    group.bench_function("and_not_cardinality", |b| {
        b.iter(|| black_box(bitmap_ops::and_not_cardinality(&bitmap1, &bitmap2)))
    });

    group.finish();
}

criterion_group!(
    benches,
    bench_bitmap_iteration,
    bench_and_operation,
    bench_or_operation,
    bench_not_operation,
    bench_advance,
    bench_cardinality_operations,
);

criterion_main!(benches);
