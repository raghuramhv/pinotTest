//! Benchmarks for scatter-gather operations.

use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use pinot_broker::transport::async_response::AsyncQueryResponse;
use pinot_broker::transport::server_channel::{ChannelConfig, ServerChannels};
use pinot_broker::types::{ColumnDataType, DataSchema, DataTable, DataValue, ServerInstance, ServerResponse, TableType};
use std::sync::Arc;
use std::time::Duration;
use tokio::runtime::Runtime;

fn create_server(name: &str) -> ServerInstance {
    ServerInstance::new(name.to_string(), 8099, TableType::Offline)
}

fn create_data_table(num_rows: usize) -> DataTable {
    let schema = DataSchema::new(
        vec!["id".to_string(), "value".to_string()],
        vec![ColumnDataType::Long, ColumnDataType::Double],
    );
    let mut table = DataTable::new(schema);

    for i in 0..num_rows {
        table.add_row(vec![
            DataValue::Long(i as i64),
            DataValue::Double(i as f64 * 1.5),
        ]);
    }

    table
}

fn bench_async_response(c: &mut Criterion) {
    let mut group = c.benchmark_group("async_response");

    for num_servers in [2, 5, 10, 20] {
        group.throughput(Throughput::Elements(num_servers as u64));

        group.bench_with_input(
            BenchmarkId::new("receive_responses", num_servers),
            &num_servers,
            |b, &num_servers| {
                b.iter(|| {
                    let response = AsyncQueryResponse::new(1, num_servers, Duration::from_secs(60));

                    for i in 0..num_servers {
                        let server = create_server(&format!("host{}", i));
                        let table = create_data_table(100);
                        response.receive_response(ServerResponse::success(server, table, 1000));
                    }

                    black_box(response.num_responses())
                })
            },
        );

        group.bench_with_input(
            BenchmarkId::new("get_responses", num_servers),
            &num_servers,
            |b, &num_servers| {
                let response = AsyncQueryResponse::new(1, num_servers, Duration::from_secs(60));

                for i in 0..num_servers {
                    let server = create_server(&format!("host{}", i));
                    let table = create_data_table(100);
                    response.receive_response(ServerResponse::success(server, table, 1000));
                }

                b.iter(|| {
                    black_box(response.get_responses())
                })
            },
        );
    }

    group.finish();
}

fn bench_async_wait(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();
    let mut group = c.benchmark_group("async_wait");

    for num_servers in [2, 5, 10] {
        group.throughput(Throughput::Elements(num_servers as u64));

        group.bench_with_input(
            BenchmarkId::new("wait_for_responses", num_servers),
            &num_servers,
            |b, &num_servers| {
                b.iter(|| {
                    rt.block_on(async {
                        let response = Arc::new(AsyncQueryResponse::new(
                            1,
                            num_servers,
                            Duration::from_secs(60),
                        ));

                        // Spawn tasks to send responses
                        let response_clone = response.clone();
                        tokio::spawn(async move {
                            for i in 0..num_servers {
                                let server = create_server(&format!("host{}", i));
                                let table = create_data_table(100);
                                response_clone.receive_response(ServerResponse::success(
                                    server, table, 1000,
                                ));
                            }
                        });

                        // Wait for responses
                        response.wait_for_responses().await.unwrap();
                        black_box(response.num_responses())
                    })
                })
            },
        );
    }

    group.finish();
}

fn bench_channel_creation(c: &mut Criterion) {
    let mut group = c.benchmark_group("channel_creation");

    let config = ChannelConfig::default();
    let channels = ServerChannels::new(config);

    group.bench_function("get_or_create_channel", |b| {
        let mut i = 0;
        b.iter(|| {
            i += 1;
            let server = create_server(&format!("host{}", i % 100));
            black_box(channels.get_or_create(&server))
        })
    });

    // Pre-create channels
    for i in 0..100 {
        let server = create_server(&format!("existing_host{}", i));
        channels.get_or_create(&server);
    }

    group.bench_function("get_existing_channel", |b| {
        let mut i = 0;
        b.iter(|| {
            i = (i + 1) % 100;
            let server = create_server(&format!("existing_host{}", i));
            black_box(channels.get_or_create(&server))
        })
    });

    group.finish();
}

fn bench_response_status(c: &mut Criterion) {
    let mut group = c.benchmark_group("response_status");

    for num_servers in [5, 10, 20] {
        let response = AsyncQueryResponse::new(1, num_servers, Duration::from_secs(60));

        // Receive all responses
        for i in 0..num_servers {
            let server = create_server(&format!("host{}", i));
            let table = create_data_table(100);
            response.receive_response(ServerResponse::success(server, table, 1000));
        }

        group.bench_with_input(
            BenchmarkId::new("num_successful", num_servers),
            &num_servers,
            |b, _| {
                b.iter(|| black_box(response.num_successful()))
            },
        );

        group.bench_with_input(
            BenchmarkId::new("is_complete", num_servers),
            &num_servers,
            |b, _| {
                b.iter(|| black_box(response.is_complete()))
            },
        );

        group.bench_with_input(
            BenchmarkId::new("get_failed_servers", num_servers),
            &num_servers,
            |b, _| {
                b.iter(|| black_box(response.get_failed_servers()))
            },
        );
    }

    group.finish();
}

fn bench_data_table_creation(c: &mut Criterion) {
    let mut group = c.benchmark_group("data_table_creation");

    for num_rows in [100, 1000, 10000] {
        group.throughput(Throughput::Elements(num_rows as u64));
        group.bench_with_input(
            BenchmarkId::new("create_table", num_rows),
            &num_rows,
            |b, &num_rows| {
                b.iter(|| black_box(create_data_table(num_rows)))
            },
        );
    }

    group.finish();
}

criterion_group!(
    benches,
    bench_async_response,
    bench_async_wait,
    bench_channel_creation,
    bench_response_status,
    bench_data_table_creation,
);
criterion_main!(benches);
