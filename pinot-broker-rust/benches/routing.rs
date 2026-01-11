//! Benchmarks for routing operations.

use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use pinot_broker::config::RoutingConfig;
use pinot_broker::routing::instance_selector::{BalancedInstanceSelector, InstanceSelector, HashBasedInstanceSelector};
use pinot_broker::routing::RoutingManager;
use pinot_broker::types::{BrokerRequest, SegmentsToQuery, ServerInstance, TableType};
use std::collections::HashMap;

fn create_segment_mapping(num_segments: usize, num_servers: usize) -> HashMap<String, Vec<ServerInstance>> {
    let servers: Vec<ServerInstance> = (0..num_servers)
        .map(|i| ServerInstance::new(format!("host{}", i), 8099, TableType::Offline))
        .collect();

    let mut mapping = HashMap::new();
    for i in 0..num_segments {
        // Each segment has 2 replicas on different servers
        let replicas = vec![
            servers[i % num_servers].clone(),
            servers[(i + 1) % num_servers].clone(),
        ];
        mapping.insert(format!("segment_{}", i), replicas);
    }
    mapping
}

fn bench_instance_selection(c: &mut Criterion) {
    let mut group = c.benchmark_group("instance_selection");

    for num_segments in [10, 100, 1000, 10000] {
        let mapping = create_segment_mapping(num_segments, 10);
        let segments: Vec<String> = mapping.keys().cloned().collect();

        // Balanced selector
        let balanced_selector = BalancedInstanceSelector::new();
        group.throughput(Throughput::Elements(num_segments as u64));
        group.bench_with_input(
            BenchmarkId::new("balanced", num_segments),
            &num_segments,
            |b, _| {
                b.iter(|| {
                    black_box(balanced_selector.select(&segments, &mapping, 1).unwrap())
                })
            },
        );

        // Hash-based selector
        let hash_selector = HashBasedInstanceSelector::new();
        group.bench_with_input(
            BenchmarkId::new("hash_based", num_segments),
            &num_segments,
            |b, _| {
                b.iter(|| {
                    black_box(hash_selector.select(&segments, &mapping, 1).unwrap())
                })
            },
        );
    }

    group.finish();
}

fn bench_routing_manager(c: &mut Criterion) {
    let mut group = c.benchmark_group("routing_manager");

    for num_segments in [100, 1000, 10000] {
        let config = RoutingConfig::default();
        let manager = RoutingManager::new(config);

        // Setup table and segments
        manager.register_table("myTable".to_string());

        let servers: Vec<ServerInstance> = (0..10)
            .map(|i| ServerInstance::new(format!("host{}", i), 8099, TableType::Offline))
            .collect();

        for i in 0..num_segments {
            let replicas = vec![
                servers[i % 10].clone(),
                servers[(i + 1) % 10].clone(),
            ];
            manager.update_segment_mapping("myTable", format!("segment_{}", i), replicas);
        }

        let request = BrokerRequest::new(
            1,
            "SELECT * FROM myTable".to_string(),
            "myTable".to_string(),
        );

        group.throughput(Throughput::Elements(num_segments as u64));
        group.bench_with_input(
            BenchmarkId::new("get_routing_table", num_segments),
            &num_segments,
            |b, _| {
                b.iter(|| {
                    black_box(manager.get_routing_table(&request, 1).unwrap())
                })
            },
        );
    }

    group.finish();
}

fn bench_table_registration(c: &mut Criterion) {
    let mut group = c.benchmark_group("table_registration");

    let config = RoutingConfig::default();
    let manager = RoutingManager::new(config);

    group.bench_function("register_table", |b| {
        let mut i = 0;
        b.iter(|| {
            i += 1;
            manager.register_table(format!("table_{}", i));
        })
    });

    // Pre-register tables for has_table benchmark
    for i in 0..1000 {
        manager.register_table(format!("existing_table_{}", i));
    }

    group.bench_function("has_table", |b| {
        let mut i = 0;
        b.iter(|| {
            i = (i + 1) % 1000;
            black_box(manager.has_table(&format!("existing_table_{}", i)))
        })
    });

    group.finish();
}

criterion_group!(
    benches,
    bench_instance_selection,
    bench_routing_manager,
    bench_table_registration,
);
criterion_main!(benches);
