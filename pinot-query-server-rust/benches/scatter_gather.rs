//! Scatter-gather performance benchmarks

use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use std::sync::Arc;
use std::time::Duration;

use pinot_query_server::block::{
    BlockId, BlockSchema, ColumnData, ColumnSchema, ColumnType, DataBlock, DataBlockBuilder, MseBlock,
};
use pinot_query_server::exchange::{BlockExchange, BlockExchangeBuilder, ExchangeType};
use pinot_query_server::mailbox::{MailboxId, MailboxService, ReceivingMailbox, SendingMailbox};
use pinot_query_server::config::QueryServerConfig;

fn create_test_schema() -> Arc<BlockSchema> {
    Arc::new(BlockSchema::new(vec![
        ColumnSchema {
            name: "id".to_string(),
            data_type: ColumnType::Long,
            nullable: false,
        },
        ColumnSchema {
            name: "value".to_string(),
            data_type: ColumnType::Double,
            nullable: false,
        },
        ColumnSchema {
            name: "name".to_string(),
            data_type: ColumnType::String,
            nullable: false,
        },
    ]))
}

fn create_data_block(num_rows: usize, sequence: u64) -> DataBlock {
    let schema = create_test_schema();
    let id = BlockId::new("benchmark", 0, 0, sequence);

    let ids: Vec<i64> = (0..num_rows as i64).collect();
    let values: Vec<f64> = (0..num_rows).map(|i| i as f64 * 1.5).collect();
    let names: Vec<String> = (0..num_rows).map(|i| format!("name_{}", i)).collect();

    DataBlockBuilder::new(id, schema)
        .add_long_column(ids)
        .add_double_column(values)
        .add_string_column(names)
        .build()
}

fn create_mailbox_pair(idx: usize) -> (Arc<SendingMailbox>, Arc<ReceivingMailbox>) {
    let id = MailboxId::new("bench", 0, 0, 1, idx as i32);
    let receiver = Arc::new(ReceivingMailbox::new(id.clone(), 100));
    let sender = Arc::new(SendingMailbox::new_local(id, receiver.clone()));
    (sender, receiver)
}

fn bench_block_serialization(c: &mut Criterion) {
    let mut group = c.benchmark_group("block_serialization");

    for num_rows in [100, 1000, 10000, 100000] {
        let block = create_data_block(num_rows, 0);

        group.throughput(Throughput::Elements(num_rows as u64));
        group.bench_with_input(
            BenchmarkId::new("serialize", num_rows),
            &block,
            |b, block| {
                b.iter(|| {
                    let serialized = black_box(block.serialize().unwrap());
                    black_box(serialized)
                });
            },
        );

        let serialized = block.serialize().unwrap();
        group.bench_with_input(
            BenchmarkId::new("deserialize", num_rows),
            &serialized,
            |b, data| {
                b.iter(|| {
                    let block = black_box(DataBlock::deserialize(data).unwrap());
                    black_box(block)
                });
            },
        );
    }

    group.finish();
}

fn bench_mailbox_throughput(c: &mut Criterion) {
    let mut group = c.benchmark_group("mailbox_throughput");
    group.measurement_time(Duration::from_secs(10));

    for num_rows in [100, 1000, 10000] {
        let (sender, receiver) = create_mailbox_pair(0);
        let block = create_data_block(num_rows, 0);
        let mse_block = MseBlock::Data(block);

        group.throughput(Throughput::Elements(num_rows as u64));
        group.bench_with_input(
            BenchmarkId::new("send_receive", num_rows),
            &mse_block,
            |b, block| {
                b.iter(|| {
                    // Send block
                    sender.send(block.clone(), Duration::from_secs(1)).unwrap();
                    // Receive block
                    let received = receiver.receive(Duration::from_secs(1)).unwrap();
                    black_box(received)
                });
            },
        );
    }

    group.finish();
}

fn bench_exchange_types(c: &mut Criterion) {
    let mut group = c.benchmark_group("exchange_types");

    for num_destinations in [2, 4, 8, 16] {
        let (senders, receivers): (Vec<_>, Vec<_>) = (0..num_destinations)
            .map(|i| create_mailbox_pair(i))
            .unzip();

        let block = create_data_block(1000, 0);
        let mse_block = MseBlock::Data(block);

        // Singleton
        {
            let exchange = BlockExchangeBuilder::new(ExchangeType::Singleton)
                .with_mailboxes(senders.clone())
                .build();

            group.bench_with_input(
                BenchmarkId::new("singleton", num_destinations),
                &mse_block,
                |b, block| {
                    b.iter(|| {
                        exchange.send(block.clone()).unwrap();
                        // Drain receiver
                        receivers[0].poll();
                    });
                },
            );
        }

        // Random
        {
            let exchange = BlockExchangeBuilder::new(ExchangeType::RandomDistributed)
                .with_mailboxes(senders.clone())
                .build();

            group.bench_with_input(
                BenchmarkId::new("random", num_destinations),
                &mse_block,
                |b, block| {
                    b.iter(|| {
                        exchange.send(block.clone()).unwrap();
                        // Drain all receivers
                        for r in &receivers {
                            while r.poll().is_some() {}
                        }
                    });
                },
            );
        }

        // Broadcast
        {
            let exchange = BlockExchangeBuilder::new(ExchangeType::BroadcastDistributed)
                .with_mailboxes(senders.clone())
                .build();

            group.bench_with_input(
                BenchmarkId::new("broadcast", num_destinations),
                &mse_block,
                |b, block| {
                    b.iter(|| {
                        exchange.send(block.clone()).unwrap();
                        // Drain all receivers
                        for r in &receivers {
                            while r.poll().is_some() {}
                        }
                    });
                },
            );
        }

        // Hash distributed
        {
            let exchange = BlockExchangeBuilder::new(ExchangeType::HashDistributed)
                .with_mailboxes(senders.clone())
                .with_hash_keys(vec![0]) // Hash on first column
                .build();

            group.bench_with_input(
                BenchmarkId::new("hash", num_destinations),
                &mse_block,
                |b, block| {
                    b.iter(|| {
                        exchange.send(block.clone()).unwrap();
                        // Drain all receivers
                        for r in &receivers {
                            while r.poll().is_some() {}
                        }
                    });
                },
            );
        }
    }

    group.finish();
}

fn bench_multi_stream_consume(c: &mut Criterion) {
    let mut group = c.benchmark_group("multi_stream_consume");

    for num_sources in [2, 4, 8, 16] {
        let receivers: Vec<_> = (0..num_sources)
            .map(|i| {
                let id = MailboxId::new("bench", i as i32, 0, 0, 0);
                Arc::new(ReceivingMailbox::new(id, 100))
            })
            .collect();

        // Pre-fill mailboxes
        for (i, rx) in receivers.iter().enumerate() {
            for j in 0..10 {
                let block = create_data_block(100, (i * 10 + j) as u64);
                let mse = MseBlock::Data(block);
                rx.sender().send(mse).unwrap();
            }
        }

        let consumer = pinot_query_server::mailbox::BlockingMultiStreamConsumer::new(
            receivers.iter().cloned().collect()
        );

        group.bench_with_input(
            BenchmarkId::new("consume", num_sources),
            &consumer,
            |b, consumer| {
                b.iter(|| {
                    if let Ok(Some(block)) = consumer.read_blocking(Duration::from_millis(1)) {
                        black_box(block);
                    }
                });
            },
        );
    }

    group.finish();
}

criterion_group!(
    benches,
    bench_block_serialization,
    bench_mailbox_throughput,
    bench_exchange_types,
    bench_multi_stream_consume,
);
criterion_main!(benches);
