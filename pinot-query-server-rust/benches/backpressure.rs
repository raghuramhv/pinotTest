//! Memory backpressure benchmarks

use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use std::sync::Arc;
use std::time::Duration;

use pinot_query_server::backpressure::{MemoryBackpressureManager, OperatorBackpressure};
use pinot_query_server::config::MemoryConfig;
use pinot_query_server::scheduler::{QueryInfo, QueryPriority, QueryScheduler};
use pinot_query_server::config::QueryServerConfig;

fn bench_pressure_check(c: &mut Criterion) {
    let mut group = c.benchmark_group("pressure_check");

    let config = MemoryConfig::default();
    let manager = Arc::new(MemoryBackpressureManager::new(config));

    group.bench_function("get_pressure_level", |b| {
        b.iter(|| {
            let level = manager.pressure_level();
            black_box(level)
        });
    });

    group.bench_function("should_throttle", |b| {
        b.iter(|| {
            let should = manager.should_throttle();
            black_box(should)
        });
    });

    group.bench_function("memory_stats", |b| {
        b.iter(|| {
            let stats = manager.memory_stats();
            black_box(stats)
        });
    });

    group.bench_function("update_stats", |b| {
        b.iter(|| {
            let stats = manager.update_stats();
            black_box(stats)
        });
    });

    group.finish();
}

fn bench_reservation(c: &mut Criterion) {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let mut group = c.benchmark_group("reservation");

    let config = MemoryConfig::default();
    let manager = Arc::new(MemoryBackpressureManager::new(config));

    for size in [1024, 1024 * 1024, 10 * 1024 * 1024] {
        group.throughput(Throughput::Bytes(size as u64));
        group.bench_with_input(
            BenchmarkId::new("try_reserve", size),
            &size,
            |b, &size| {
                b.iter(|| {
                    let reservation = manager.try_reserve(size).unwrap();
                    black_box(&reservation);
                    drop(reservation);
                });
            },
        );

        group.bench_with_input(
            BenchmarkId::new("async_reserve", size),
            &size,
            |b, &size| {
                let mgr = manager.clone();
                b.to_async(&rt).iter(|| async {
                    let reservation = mgr.reserve(size).await.unwrap();
                    black_box(&reservation);
                    drop(reservation);
                });
            },
        );
    }

    group.finish();
}

fn bench_operator_backpressure(c: &mut Criterion) {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let mut group = c.benchmark_group("operator_backpressure");

    let config = MemoryConfig::default();
    let manager = Arc::new(MemoryBackpressureManager::new(config));
    let op_bp = OperatorBackpressure::new(manager, "TestOperator");

    for batch_size in [100, 1000, 10000] {
        let bytes = batch_size * 8; // Assume 8 bytes per row

        group.throughput(Throughput::Elements(batch_size as u64));
        group.bench_with_input(
            BenchmarkId::new("record_and_check", batch_size),
            &(bytes, batch_size),
            |b, &(bytes, rows)| {
                b.to_async(&rt).iter(|| async {
                    op_bp.record_and_check(bytes, rows).await;
                });
            },
        );
    }

    group.finish();
}

fn bench_scheduler_operations(c: &mut Criterion) {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let mut group = c.benchmark_group("scheduler");

    let config = Arc::new(QueryServerConfig::default());
    let bp_config = MemoryConfig::default();
    let backpressure = Arc::new(MemoryBackpressureManager::new(bp_config));
    let scheduler = Arc::new(QueryScheduler::new(config, backpressure));

    group.bench_function("submit_query", |b| {
        let mut counter = 0u64;
        let sched = scheduler.clone();
        b.iter(|| {
            counter += 1;
            let query = Arc::new(QueryInfo::new(
                format!("q{}", counter),
                QueryPriority::Normal,
                1024,
                3,
                Duration::from_secs(30),
            ));
            sched.submit(query).unwrap();
        });
    });

    // Pre-submit some queries
    for i in 0..100 {
        let query = Arc::new(QueryInfo::new(
            format!("preload_{}", i),
            QueryPriority::Normal,
            1024,
            3,
            Duration::from_secs(30),
        ));
        scheduler.submit(query).unwrap();
    }

    group.bench_function("next_query", |b| {
        let sched = scheduler.clone();
        b.to_async(&rt).iter(|| async {
            if let Some(query) = sched.next_query().await {
                // Complete the query so semaphore is released
                sched.complete_query(&query.id, true, None);
                black_box(query);
            }
        });
    });

    group.bench_function("get_query", |b| {
        let sched = scheduler.clone();
        b.iter(|| {
            let query = sched.get_query("preload_50");
            black_box(query);
        });
    });

    group.bench_function("running_count", |b| {
        let sched = scheduler.clone();
        b.iter(|| {
            let count = sched.running_count();
            black_box(count);
        });
    });

    group.finish();
}

fn bench_concurrent_reservations(c: &mut Criterion) {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let mut group = c.benchmark_group("concurrent_reservations");

    let config = MemoryConfig::default();
    let manager = Arc::new(MemoryBackpressureManager::new(config));

    for num_concurrent in [2, 4, 8, 16] {
        group.bench_with_input(
            BenchmarkId::new("concurrent_reserve", num_concurrent),
            &num_concurrent,
            |b, &num_concurrent| {
                let mgr = manager.clone();
                b.to_async(&rt).iter(|| async {
                    let mut handles = Vec::new();
                    for _ in 0..num_concurrent {
                        let m = mgr.clone();
                        handles.push(tokio::spawn(async move {
                            let reservation = m.reserve(1024).await.unwrap();
                            tokio::time::sleep(Duration::from_micros(10)).await;
                            drop(reservation);
                        }));
                    }
                    for h in handles {
                        h.await.unwrap();
                    }
                });
            },
        );
    }

    group.finish();
}

criterion_group!(
    benches,
    bench_pressure_check,
    bench_reservation,
    bench_operator_backpressure,
    bench_scheduler_operations,
    bench_concurrent_reservations,
);
criterion_main!(benches);
