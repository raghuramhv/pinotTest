//! Benchmarks for reduce operations.

use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use pinot_broker::reduce::aggregator::{AggregationFunction, Aggregator, SumAggregator, CountAggregator};
use pinot_broker::reduce::merger::DataTableMerger;
use pinot_broker::reduce::{BrokerReduceService, ReduceConfig};
use pinot_broker::types::{ColumnDataType, DataSchema, DataTable, DataValue, ServerInstance, ServerResponse, TableType};

fn create_data_table(num_rows: usize, num_cols: usize) -> DataTable {
    let column_names: Vec<String> = (0..num_cols).map(|i| format!("col{}", i)).collect();
    let column_types: Vec<ColumnDataType> = (0..num_cols)
        .map(|i| if i == 0 { ColumnDataType::String } else { ColumnDataType::Double })
        .collect();

    let schema = DataSchema::new(column_names, column_types);
    let mut table = DataTable::new(schema);

    for i in 0..num_rows {
        let mut row = Vec::with_capacity(num_cols);
        row.push(DataValue::String(format!("group_{}", i % 100)));
        for j in 1..num_cols {
            row.push(DataValue::Double((i * j) as f64));
        }
        table.add_row(row);
    }

    table
}

fn create_server(name: &str) -> ServerInstance {
    ServerInstance::new(name.to_string(), 8099, TableType::Offline)
}

fn bench_data_table_merge(c: &mut Criterion) {
    let mut group = c.benchmark_group("data_table_merge");

    for num_tables in [2, 5, 10] {
        for rows_per_table in [100, 1000, 10000] {
            let merger = DataTableMerger::new(100_000);
            let total_rows = num_tables * rows_per_table;

            group.throughput(Throughput::Elements(total_rows as u64));
            group.bench_with_input(
                BenchmarkId::new(format!("tables_{}", num_tables), rows_per_table),
                &rows_per_table,
                |b, &rows_per_table| {
                    b.iter(|| {
                        let tables: Vec<DataTable> = (0..num_tables)
                            .map(|_| create_data_table(rows_per_table, 5))
                            .collect();
                        black_box(merger.merge(tables).unwrap())
                    })
                },
            );
        }
    }

    group.finish();
}

fn bench_ordered_merge(c: &mut Criterion) {
    let mut group = c.benchmark_group("ordered_merge");

    for num_tables in [2, 5, 10] {
        let rows_per_table = 1000;
        let merger = DataTableMerger::new(100_000);

        group.throughput(Throughput::Elements((num_tables * rows_per_table) as u64));
        group.bench_with_input(
            BenchmarkId::new("merge_ordered", num_tables),
            &num_tables,
            |b, &num_tables| {
                b.iter(|| {
                    let tables: Vec<DataTable> = (0..num_tables)
                        .map(|_| create_data_table(rows_per_table, 5))
                        .collect();
                    black_box(merger.merge_ordered(tables, &[(1, true)]).unwrap())
                })
            },
        );
    }

    group.finish();
}

fn bench_aggregation(c: &mut Criterion) {
    let mut group = c.benchmark_group("aggregation");

    for num_rows in [1000, 10000, 100000] {
        // Sum aggregation
        group.throughput(Throughput::Elements(num_rows as u64));
        group.bench_with_input(
            BenchmarkId::new("sum", num_rows),
            &num_rows,
            |b, &num_rows| {
                b.iter(|| {
                    let table = create_data_table(num_rows, 5);
                    let aggregator = Aggregator::new(
                        vec![0],
                        vec![(1, AggregationFunction::Sum)],
                    );
                    black_box(aggregator.aggregate(vec![table]).unwrap())
                })
            },
        );

        // Multiple aggregations
        group.bench_with_input(
            BenchmarkId::new("multi_agg", num_rows),
            &num_rows,
            |b, &num_rows| {
                b.iter(|| {
                    let table = create_data_table(num_rows, 5);
                    let aggregator = Aggregator::new(
                        vec![0],
                        vec![
                            (1, AggregationFunction::Sum),
                            (2, AggregationFunction::Min),
                            (3, AggregationFunction::Max),
                            (4, AggregationFunction::Avg),
                        ],
                    );
                    black_box(aggregator.aggregate(vec![table]).unwrap())
                })
            },
        );
    }

    group.finish();
}

fn bench_reduce_service(c: &mut Criterion) {
    let mut group = c.benchmark_group("reduce_service");

    let config = ReduceConfig {
        max_rows: 100_000,
        enable_parallel: true,
        ..Default::default()
    };
    let service = BrokerReduceService::new(config);

    for num_servers in [2, 5, 10] {
        let rows_per_server = 1000;

        group.throughput(Throughput::Elements((num_servers * rows_per_server) as u64));
        group.bench_with_input(
            BenchmarkId::new("reduce", num_servers),
            &num_servers,
            |b, &num_servers| {
                b.iter(|| {
                    let responses: Vec<(ServerInstance, ServerResponse)> = (0..num_servers)
                        .map(|i| {
                            let server = create_server(&format!("host{}", i));
                            let table = create_data_table(rows_per_server, 5);
                            (server.clone(), ServerResponse::success(server, table, 1000))
                        })
                        .collect();
                    black_box(service.reduce(responses, num_servers).unwrap())
                })
            },
        );
    }

    group.finish();
}

fn bench_simple_aggregators(c: &mut Criterion) {
    let mut group = c.benchmark_group("simple_aggregators");

    let values: Vec<DataValue> = (0..10000)
        .map(|i| DataValue::Double(i as f64))
        .collect();

    // Sum aggregator
    group.throughput(Throughput::Elements(10000));
    group.bench_function("sum_aggregator", |b| {
        b.iter(|| {
            let mut agg = SumAggregator::new();
            for v in &values {
                agg.add(v);
            }
            black_box(agg.result())
        })
    });

    // Count aggregator
    group.bench_function("count_aggregator", |b| {
        b.iter(|| {
            let mut agg = CountAggregator::new();
            for _ in &values {
                agg.add();
            }
            black_box(agg.result())
        })
    });

    group.finish();
}

criterion_group!(
    benches,
    bench_data_table_merge,
    bench_ordered_merge,
    bench_aggregation,
    bench_reduce_service,
    bench_simple_aggregators,
);
criterion_main!(benches);
