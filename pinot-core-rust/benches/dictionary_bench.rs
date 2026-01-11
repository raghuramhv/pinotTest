//! Benchmarks for dictionary operations

use criterion::{black_box, criterion_group, criterion_main, Criterion, BenchmarkId};
use pinot_core::dictionary::*;

fn bench_int_dictionary_lookup(c: &mut Criterion) {
    let values: Vec<i32> = (0..10000).collect();
    let dict = OnHeapIntDictionary::new(values);

    c.bench_function("int_dict_lookup", |b| {
        b.iter(|| {
            for i in 0..1000 {
                black_box(dict.index_of_int(i));
            }
        })
    });
}

fn bench_int_dictionary_decode(c: &mut Criterion) {
    let values: Vec<i32> = (0..10000).collect();
    let dict = OnHeapIntDictionary::new(values);

    c.bench_function("int_dict_decode", |b| {
        b.iter(|| {
            for i in 0..1000 {
                black_box(dict.get_int(i).unwrap());
            }
        })
    });
}

fn bench_string_dictionary_lookup(c: &mut Criterion) {
    let values: Vec<String> = (0..10000).map(|i| format!("value_{:05}", i)).collect();
    let dict = OnHeapStringDictionary::new(values);

    c.bench_function("string_dict_lookup", |b| {
        b.iter(|| {
            for i in 0..1000 {
                black_box(dict.index_of_string(&format!("value_{:05}", i)));
            }
        })
    });
}

fn bench_mutable_int_dictionary_insert(c: &mut Criterion) {
    c.bench_function("mutable_int_dict_insert", |b| {
        b.iter(|| {
            let mut dict = IntOnHeapMutableDictionary::new();
            for i in 0..1000 {
                black_box(dict.index(DictionaryValue::Int(i)));
            }
        })
    });
}

fn bench_int_dictionary_range_query(c: &mut Criterion) {
    let values: Vec<i32> = (0..10000).collect();
    let dict = OnHeapIntDictionary::new(values);

    let mut group = c.benchmark_group("int_dict_range");

    for range_size in [100, 500, 1000, 2000].iter() {
        group.bench_with_input(
            BenchmarkId::from_parameter(range_size),
            range_size,
            |b, &size| {
                b.iter(|| {
                    let lower = DictionaryValue::Int(1000);
                    let upper = DictionaryValue::Int(1000 + size);
                    black_box(dict.get_dict_ids_in_range(
                        Some(&lower),
                        Some(&upper),
                        true,
                        true,
                    ))
                })
            },
        );
    }

    group.finish();
}

criterion_group!(
    benches,
    bench_int_dictionary_lookup,
    bench_int_dictionary_decode,
    bench_string_dictionary_lookup,
    bench_mutable_int_dictionary_insert,
    bench_int_dictionary_range_query,
);

criterion_main!(benches);
