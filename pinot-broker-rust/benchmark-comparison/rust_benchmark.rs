//! Rust benchmark for direct comparison with Java JMH benchmark
//! Run with: cargo bench --bench comparison

use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};

// ============== Data Structures ==============

#[derive(Clone, Debug, Hash, PartialEq, Eq)]
struct ServerInstance {
    name: String,
    host: String,
    port: u16,
}

struct DataTable {
    rows: Vec<Vec<DataValue>>,
}

#[derive(Clone, Debug)]
enum DataValue {
    String(String),
    Double(f64),
}

struct RoutingTable {
    segment_to_servers: HashMap<String, Vec<ServerInstance>>,
}

impl RoutingTable {
    fn select_servers_balanced(&self, segments: &[String]) -> HashMap<ServerInstance, Vec<String>> {
        let mut result: HashMap<ServerInstance, Vec<String>> = HashMap::new();
        let mut current_load: HashMap<&ServerInstance, usize> = HashMap::new();

        for segment in segments {
            if let Some(candidates) = self.segment_to_servers.get(segment) {
                if candidates.is_empty() {
                    continue;
                }

                // Select server with minimum load
                let selected = candidates
                    .iter()
                    .min_by_key(|s| current_load.get(s).copied().unwrap_or(0))
                    .unwrap();

                result
                    .entry(selected.clone())
                    .or_insert_with(Vec::new)
                    .push(segment.clone());
                *current_load.entry(selected).or_insert(0) += 1;
            }
        }

        result
    }

    fn select_servers_hash_based(&self, segments: &[String]) -> HashMap<ServerInstance, Vec<String>> {
        let mut result: HashMap<ServerInstance, Vec<String>> = HashMap::new();

        for segment in segments {
            if let Some(candidates) = self.segment_to_servers.get(segment) {
                if candidates.is_empty() {
                    continue;
                }

                // Select server based on hash
                let hash = fxhash::hash64(segment);
                let selected = &candidates[(hash as usize) % candidates.len()];

                result
                    .entry(selected.clone())
                    .or_insert_with(Vec::new)
                    .push(segment.clone());
            }
        }

        result
    }
}

// ============== Setup Functions ==============

fn setup_routing(num_segments: usize) -> (Vec<String>, RoutingTable) {
    use rand::{Rng, SeedableRng};
    let mut rng = rand::rngs::StdRng::seed_from_u64(42);

    // Create segments
    let segments: Vec<String> = (0..num_segments).map(|i| format!("segment_{}", i)).collect();

    // Create servers
    let servers: Vec<ServerInstance> = (0..10)
        .map(|i| ServerInstance {
            name: format!("server_{}", i),
            host: format!("192.168.1.{}", i),
            port: 8099 + i as u16,
        })
        .collect();

    // Create segment to server mapping
    let mut segment_to_servers: HashMap<String, Vec<ServerInstance>> = HashMap::new();
    for segment in &segments {
        let mut seg_servers = Vec::new();
        let mut used = std::collections::HashSet::new();
        while seg_servers.len() < 2 {
            let idx = rng.gen_range(0..servers.len());
            if !used.contains(&idx) {
                used.insert(idx);
                seg_servers.push(servers[idx].clone());
            }
        }
        segment_to_servers.insert(segment.clone(), seg_servers);
    }

    (segments, RoutingTable { segment_to_servers })
}

fn setup_data_tables(num_tables: usize, rows_per_table: usize, num_columns: usize) -> Vec<DataTable> {
    use rand::{Rng, SeedableRng};
    let mut rng = rand::rngs::StdRng::seed_from_u64(42);

    (0..num_tables)
        .map(|_| {
            let rows: Vec<Vec<DataValue>> = (0..rows_per_table)
                .map(|_| {
                    (0..num_columns)
                        .map(|c| {
                            if c < 3 {
                                DataValue::String(format!("dim_{}", rng.gen_range(0..1000)))
                            } else {
                                DataValue::Double(rng.gen::<f64>() * 1000.0)
                            }
                        })
                        .collect()
                })
                .collect();
            DataTable { rows }
        })
        .collect()
}

fn setup_group_data(num_groups: usize) -> HashMap<String, Vec<f64>> {
    use rand::{Rng, SeedableRng};
    let mut rng = rand::rngs::StdRng::seed_from_u64(42);

    (0..num_groups)
        .map(|g| {
            let key = format!("group_{}", g);
            let values: Vec<f64> = (0..100).map(|_| rng.gen::<f64>() * 1000.0).collect();
            (key, values)
        })
        .collect()
}

// ============== Benchmarks ==============

fn routing_benchmarks(c: &mut Criterion) {
    let mut group = c.benchmark_group("routing");

    for num_segments in [100, 1000, 10000] {
        let (segments, routing_table) = setup_routing(num_segments);

        group.throughput(Throughput::Elements(num_segments as u64));

        group.bench_with_input(
            BenchmarkId::new("balanced", num_segments),
            &(&segments, &routing_table),
            |b, (segments, routing_table)| {
                b.iter(|| routing_table.select_servers_balanced(black_box(segments)))
            },
        );

        group.bench_with_input(
            BenchmarkId::new("hash_based", num_segments),
            &(&segments, &routing_table),
            |b, (segments, routing_table)| {
                b.iter(|| routing_table.select_servers_hash_based(black_box(segments)))
            },
        );
    }

    group.finish();
}

fn reduce_benchmarks(c: &mut Criterion) {
    let mut group = c.benchmark_group("reduce");

    let data_tables = setup_data_tables(10, 10000, 10);
    let total_rows = 10 * 10000;

    group.throughput(Throughput::Elements(total_rows as u64));

    group.bench_function("sequential_merge", |b| {
        b.iter(|| {
            let result: Vec<&Vec<DataValue>> = data_tables
                .iter()
                .flat_map(|table| table.rows.iter())
                .collect();
            black_box(result)
        })
    });

    group.bench_function("parallel_merge", |b| {
        use rayon::prelude::*;
        b.iter(|| {
            let result: Vec<&Vec<DataValue>> = data_tables
                .par_iter()
                .flat_map(|table| table.rows.par_iter())
                .collect();
            black_box(result)
        })
    });

    group.finish();
}

fn aggregation_benchmarks(c: &mut Criterion) {
    let mut group = c.benchmark_group("aggregation");

    for num_groups in [1000, 10000, 100000] {
        let group_data = setup_group_data(num_groups);

        group.throughput(Throughput::Elements((num_groups * 100) as u64));

        group.bench_with_input(
            BenchmarkId::new("sum", num_groups),
            &group_data,
            |b, group_data| {
                b.iter(|| {
                    let result: HashMap<&str, f64> = group_data
                        .iter()
                        .map(|(k, values)| (k.as_str(), values.iter().sum()))
                        .collect();
                    black_box(result)
                })
            },
        );

        group.bench_with_input(
            BenchmarkId::new("parallel_sum", num_groups),
            &group_data,
            |b, group_data| {
                use rayon::prelude::*;
                b.iter(|| {
                    let result: HashMap<&str, f64> = group_data
                        .par_iter()
                        .map(|(k, values)| (k.as_str(), values.iter().sum()))
                        .collect();
                    black_box(result)
                })
            },
        );

        group.bench_with_input(
            BenchmarkId::new("min_max", num_groups),
            &group_data,
            |b, group_data| {
                b.iter(|| {
                    let result: HashMap<&str, (f64, f64)> = group_data
                        .iter()
                        .map(|(k, values)| {
                            let min = values.iter().cloned().fold(f64::MAX, f64::min);
                            let max = values.iter().cloned().fold(f64::MIN, f64::max);
                            (k.as_str(), (min, max))
                        })
                        .collect();
                    black_box(result)
                })
            },
        );
    }

    group.finish();
}

fn concurrent_benchmarks(c: &mut Criterion) {
    use dashmap::DashMap;
    use rayon::prelude::*;

    let mut group = c.benchmark_group("concurrent");

    let data_tables = setup_data_tables(10, 10000, 10);

    group.bench_function("concurrent_group_by", |b| {
        b.iter(|| {
            let result: DashMap<String, AtomicU64> = DashMap::new();
            data_tables.par_iter().for_each(|table| {
                for row in &table.rows {
                    if let DataValue::String(key) = &row[0] {
                        result
                            .entry(key.clone())
                            .or_insert_with(|| AtomicU64::new(0))
                            .fetch_add(1, Ordering::Relaxed);
                    }
                }
            });
            black_box(result)
        })
    });

    group.finish();
}

criterion_group!(
    benches,
    routing_benchmarks,
    reduce_benchmarks,
    aggregation_benchmarks,
    concurrent_benchmarks
);
criterion_main!(benches);
