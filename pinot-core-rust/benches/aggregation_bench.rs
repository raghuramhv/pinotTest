//! Benchmarks for aggregation operations

use criterion::{black_box, criterion_group, criterion_main, Criterion, BenchmarkId};
use pinot_core::aggregation::*;

fn bench_sum_aggregation(c: &mut Criterion) {
    let func = SumAggregationFunction::new();
    let values: Vec<f64> = (0..10000).map(|i| i as f64).collect();

    c.bench_function("sum_10k", |b| {
        b.iter(|| {
            let mut holder = func.create_result_holder();
            func.aggregate_double(&values, holder.as_mut(), None);
            black_box(holder.get_double())
        })
    });
}

fn bench_count_aggregation(c: &mut Criterion) {
    let func = CountAggregationFunction::new();
    let values: Vec<f64> = (0..10000).map(|i| i as f64).collect();

    c.bench_function("count_10k", |b| {
        b.iter(|| {
            let mut holder = func.create_result_holder();
            func.aggregate_double(&values, holder.as_mut(), None);
            black_box(holder.get_long())
        })
    });
}

fn bench_min_max_aggregation(c: &mut Criterion) {
    let values: Vec<f64> = (0..10000).map(|i| (i as f64 * 1.5).sin()).collect();

    let mut group = c.benchmark_group("min_max");

    group.bench_function("min", |b| {
        let func = MinAggregationFunction::new();
        b.iter(|| {
            let mut holder = func.create_result_holder();
            func.aggregate_double(&values, holder.as_mut(), None);
            black_box(holder.get_double())
        })
    });

    group.bench_function("max", |b| {
        let func = MaxAggregationFunction::new();
        b.iter(|| {
            let mut holder = func.create_result_holder();
            func.aggregate_double(&values, holder.as_mut(), None);
            black_box(holder.get_double())
        })
    });

    group.finish();
}

fn bench_avg_aggregation(c: &mut Criterion) {
    let func = AvgAggregationFunction::new();
    let values: Vec<f64> = (0..10000).map(|i| i as f64).collect();

    c.bench_function("avg_10k", |b| {
        b.iter(|| {
            let mut holder = func.create_result_holder();
            func.aggregate_double(&values, holder.as_mut(), None);
            black_box(holder.get_double())
        })
    });
}

fn bench_group_by_sum(c: &mut Criterion) {
    let func = SumAggregationFunction::new();
    let values: Vec<f64> = (0..10000).map(|i| i as f64).collect();
    let group_keys: Vec<i32> = (0..10000).map(|i| (i % 100) as i32).collect();

    c.bench_function("group_by_sum_100_groups", |b| {
        b.iter(|| {
            let mut holder = func.create_group_by_result_holder(100, 1000);
            func.aggregate_double_group_by(&values, &group_keys, holder.as_mut(), None);
            black_box(holder.get_double_result(50))
        })
    });
}

fn bench_executor(c: &mut Criterion) {
    let values: Vec<f64> = (0..10000).map(|i| i as f64).collect();

    c.bench_function("executor_all_aggs", |b| {
        b.iter(|| {
            let mut executor = AggregationExecutor::new(&[
                AggregationType::Sum,
                AggregationType::Count,
                AggregationType::Min,
                AggregationType::Max,
                AggregationType::Avg,
            ]);
            executor.aggregate_double(&values, None);
            black_box(executor.get_results().unwrap())
        })
    });
}

criterion_group!(
    benches,
    bench_sum_aggregation,
    bench_count_aggregation,
    bench_min_max_aggregation,
    bench_avg_aggregation,
    bench_group_by_sum,
    bench_executor,
);

criterion_main!(benches);
