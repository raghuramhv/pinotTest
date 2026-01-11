//! Block exchange for scatter-gather distribution
//!
//! This module implements different exchange patterns for distributing data blocks
//! across multiple workers in a query stage.

use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use rand::Rng;
use siphasher::sip::SipHasher13;

use crate::block::{BlockStats, ColumnData, DataBlock, MseBlock};
use crate::error::{QueryError, Result};
use crate::mailbox::{MailboxId, SendingMailbox};
use crate::MAX_BLOCK_SIZE_BYTES;

/// Exchange type for data distribution
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExchangeType {
    /// Send to single destination (reduce to one)
    Singleton,
    /// Random distribution across destinations
    RandomDistributed,
    /// Broadcast to all destinations
    BroadcastDistributed,
    /// Hash-based distribution by key columns
    HashDistributed,
    /// Range-based distribution
    RangeDistributed,
}

impl std::fmt::Display for ExchangeType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ExchangeType::Singleton => write!(f, "SINGLETON"),
            ExchangeType::RandomDistributed => write!(f, "RANDOM"),
            ExchangeType::BroadcastDistributed => write!(f, "BROADCAST"),
            ExchangeType::HashDistributed => write!(f, "HASH"),
            ExchangeType::RangeDistributed => write!(f, "RANGE"),
        }
    }
}

/// Exchange statistics
#[derive(Debug, Default)]
pub struct ExchangeStats {
    /// Blocks sent
    pub blocks_sent: AtomicU64,
    /// Rows sent
    pub rows_sent: AtomicU64,
    /// Bytes sent
    pub bytes_sent: AtomicU64,
    /// Blocks split due to size
    pub blocks_split: AtomicU64,
    /// Send errors
    pub send_errors: AtomicU64,
    /// Total send time (microseconds)
    pub send_time_us: AtomicU64,
    /// Backpressure wait time (microseconds)
    pub backpressure_wait_us: AtomicU64,
}

impl ExchangeStats {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn to_block_stats(&self) -> BlockStats {
        BlockStats {
            emitted_rows: self.rows_sent.load(Ordering::Relaxed),
            serialized_bytes: self.bytes_sent.load(Ordering::Relaxed),
            execution_time_ms: self.send_time_us.load(Ordering::Relaxed) / 1000,
            downstream_wait_ms: self.backpressure_wait_us.load(Ordering::Relaxed) / 1000,
            ..Default::default()
        }
    }
}

/// Block exchange for distributing data across mailboxes
pub struct BlockExchange {
    /// Exchange type
    exchange_type: ExchangeType,
    /// Destination mailboxes
    mailboxes: Vec<Arc<SendingMailbox>>,
    /// Hash key columns (for hash distribution)
    hash_key_columns: Vec<usize>,
    /// Maximum block size for splitting
    max_block_size: usize,
    /// Send timeout
    timeout: Duration,
    /// Statistics
    stats: Arc<ExchangeStats>,
    /// Round-robin counter (for random distribution)
    round_robin: AtomicUsize,
    /// Early termination received
    early_terminated: std::sync::atomic::AtomicBool,
}

impl BlockExchange {
    /// Create a new block exchange
    pub fn new(
        exchange_type: ExchangeType,
        mailboxes: Vec<Arc<SendingMailbox>>,
        hash_key_columns: Vec<usize>,
        timeout: Duration,
    ) -> Self {
        Self {
            exchange_type,
            mailboxes,
            hash_key_columns,
            max_block_size: MAX_BLOCK_SIZE_BYTES,
            timeout,
            stats: Arc::new(ExchangeStats::new()),
            round_robin: AtomicUsize::new(0),
            early_terminated: std::sync::atomic::AtomicBool::new(false),
        }
    }

    /// Set maximum block size
    pub fn with_max_block_size(mut self, size: usize) -> Self {
        self.max_block_size = size;
        self
    }

    /// Get exchange type
    pub fn exchange_type(&self) -> ExchangeType {
        self.exchange_type
    }

    /// Get statistics
    pub fn stats(&self) -> &ExchangeStats {
        &self.stats
    }

    /// Check if any destination signaled early termination
    pub fn check_early_termination(&self) -> bool {
        if self.early_terminated.load(Ordering::Relaxed) {
            return true;
        }

        for mailbox in &self.mailboxes {
            if mailbox.is_early_terminated() {
                self.early_terminated.store(true, Ordering::Relaxed);
                return true;
            }
        }
        false
    }

    /// Send a data block
    pub fn send(&self, block: MseBlock) -> Result<()> {
        if self.check_early_termination() {
            return Err(QueryError::EarlyTermination(
                "Downstream signaled early termination".to_string(),
            ));
        }

        let start = Instant::now();

        // Handle EOS blocks specially - send to only one destination for stats
        if let MseBlock::Eos { .. } = &block {
            return self.send_eos(block);
        }

        match self.exchange_type {
            ExchangeType::Singleton => self.send_singleton(block),
            ExchangeType::RandomDistributed => self.send_random(block),
            ExchangeType::BroadcastDistributed => self.send_broadcast(block),
            ExchangeType::HashDistributed => self.send_hash_distributed(block),
            ExchangeType::RangeDistributed => self.send_range_distributed(block),
        }?;

        self.stats
            .send_time_us
            .fetch_add(start.elapsed().as_micros() as u64, Ordering::Relaxed);

        Ok(())
    }

    /// Send EOS to single destination
    fn send_eos(&self, block: MseBlock) -> Result<()> {
        if self.mailboxes.is_empty() {
            return Ok(());
        }

        // Send EOS to first active mailbox
        for mailbox in &self.mailboxes {
            if !mailbox.is_closed() {
                return mailbox.send(block, self.timeout);
            }
        }

        Ok(())
    }

    /// Send to single destination (singleton)
    fn send_singleton(&self, block: MseBlock) -> Result<()> {
        if self.mailboxes.is_empty() {
            return Err(QueryError::Exchange("No destinations configured".to_string()));
        }

        self.send_to_mailbox(&self.mailboxes[0], block)
    }

    /// Send to random destination
    fn send_random(&self, block: MseBlock) -> Result<()> {
        if self.mailboxes.is_empty() {
            return Err(QueryError::Exchange("No destinations configured".to_string()));
        }

        let idx = self.round_robin.fetch_add(1, Ordering::Relaxed) % self.mailboxes.len();
        self.send_to_mailbox(&self.mailboxes[idx], block)
    }

    /// Broadcast to all destinations
    fn send_broadcast(&self, block: MseBlock) -> Result<()> {
        for mailbox in &self.mailboxes {
            // Clone the block for each destination
            self.send_to_mailbox(mailbox, block.clone())?;
        }
        Ok(())
    }

    /// Hash-distributed send
    fn send_hash_distributed(&self, block: MseBlock) -> Result<()> {
        if self.mailboxes.is_empty() {
            return Err(QueryError::Exchange("No destinations configured".to_string()));
        }

        let data_block = match &block {
            MseBlock::Data(db) => db,
            _ => return self.send_singleton(block),
        };

        if self.hash_key_columns.is_empty() || data_block.num_rows == 0 {
            return self.send_random(block);
        }

        // Partition rows by hash
        let partitions = self.partition_by_hash(data_block)?;

        for (mailbox_idx, rows) in partitions.into_iter().enumerate() {
            if rows.is_empty() {
                continue;
            }

            let partition_block = self.extract_rows(data_block, &rows)?;
            let partition_mse = MseBlock::Data(partition_block);
            self.send_to_mailbox(&self.mailboxes[mailbox_idx], partition_mse)?;
        }

        Ok(())
    }

    /// Range-distributed send
    fn send_range_distributed(&self, block: MseBlock) -> Result<()> {
        // For now, fall back to hash distribution
        self.send_hash_distributed(block)
    }

    /// Partition rows by hash of key columns
    fn partition_by_hash(&self, block: &DataBlock) -> Result<Vec<Vec<usize>>> {
        let num_partitions = self.mailboxes.len();
        let mut partitions: Vec<Vec<usize>> = vec![Vec::new(); num_partitions];

        for row_idx in 0..block.num_rows {
            let hash = self.compute_row_hash(block, row_idx);
            let partition = (hash as usize) % num_partitions;
            partitions[partition].push(row_idx);
        }

        Ok(partitions)
    }

    /// Compute hash for a row based on key columns
    fn compute_row_hash(&self, block: &DataBlock, row_idx: usize) -> u64 {
        let mut hasher = SipHasher13::new();

        for &col_idx in &self.hash_key_columns {
            if col_idx >= block.columns.len() {
                continue;
            }

            match &block.columns[col_idx] {
                ColumnData::Int(data) => {
                    if row_idx < data.len() {
                        data[row_idx].hash(&mut hasher);
                    }
                }
                ColumnData::Long(data) => {
                    if row_idx < data.len() {
                        data[row_idx].hash(&mut hasher);
                    }
                }
                ColumnData::String(data) => {
                    if row_idx < data.len() {
                        data[row_idx].hash(&mut hasher);
                    }
                }
                ColumnData::Double(data) => {
                    if row_idx < data.len() {
                        data[row_idx].to_bits().hash(&mut hasher);
                    }
                }
                ColumnData::Float(data) => {
                    if row_idx < data.len() {
                        data[row_idx].to_bits().hash(&mut hasher);
                    }
                }
                ColumnData::Bytes(data) => {
                    if row_idx < data.len() {
                        data[row_idx].hash(&mut hasher);
                    }
                }
                ColumnData::Boolean(data) => {
                    if row_idx < data.len() {
                        data[row_idx].hash(&mut hasher);
                    }
                }
                ColumnData::Nulls(_) => {}
            }
        }

        hasher.finish()
    }

    /// Extract specified rows from a block
    fn extract_rows(&self, block: &DataBlock, row_indices: &[usize]) -> Result<DataBlock> {
        let new_columns: Vec<ColumnData> = block
            .columns
            .iter()
            .map(|col| extract_column_rows(col, row_indices))
            .collect();

        let new_id = crate::block::BlockId {
            query_id: block.id.query_id.clone(),
            stage_id: block.id.stage_id,
            worker_id: block.id.worker_id,
            sequence: block.id.sequence,
        };

        Ok(DataBlock::new(new_id, block.schema.clone(), new_columns))
    }

    /// Send block to a specific mailbox with retries and splitting
    fn send_to_mailbox(&self, mailbox: &SendingMailbox, block: MseBlock) -> Result<()> {
        let bytes = block.memory_size();
        let rows = match &block {
            MseBlock::Data(db) => db.num_rows as u64,
            _ => 0,
        };

        // Split large blocks
        if bytes > self.max_block_size {
            if let MseBlock::Data(data_block) = block {
                let split_blocks = data_block.split_if_needed(self.max_block_size);
                self.stats
                    .blocks_split
                    .fetch_add(split_blocks.len() as u64 - 1, Ordering::Relaxed);

                for split_block in split_blocks {
                    let split_mse = MseBlock::Data(split_block);
                    self.send_single_block(mailbox, split_mse)?;
                }
                return Ok(());
            }
        }

        self.send_single_block(mailbox, block)?;

        self.stats.blocks_sent.fetch_add(1, Ordering::Relaxed);
        self.stats.rows_sent.fetch_add(rows, Ordering::Relaxed);
        self.stats.bytes_sent.fetch_add(bytes as u64, Ordering::Relaxed);

        Ok(())
    }

    /// Send a single block with retry logic
    fn send_single_block(&self, mailbox: &SendingMailbox, block: MseBlock) -> Result<()> {
        let mut backoff = Duration::from_millis(1);
        let max_backoff = Duration::from_millis(100);
        let mut attempts = 0;
        const MAX_ATTEMPTS: u32 = 5;

        loop {
            match mailbox.send(block.clone(), self.timeout) {
                Ok(()) => return Ok(()),
                Err(QueryError::MailboxTimeout { .. }) if attempts < MAX_ATTEMPTS => {
                    // Backpressure - wait and retry
                    self.stats
                        .backpressure_wait_us
                        .fetch_add(backoff.as_micros() as u64, Ordering::Relaxed);
                    std::thread::sleep(backoff);
                    backoff = std::cmp::min(backoff * 2, max_backoff);
                    attempts += 1;
                }
                Err(e) => {
                    self.stats.send_errors.fetch_add(1, Ordering::Relaxed);
                    return Err(e);
                }
            }
        }
    }

    /// Close all mailboxes
    pub fn close(&self) {
        for mailbox in &self.mailboxes {
            mailbox.close();
        }
    }
}

/// Extract rows from a column
fn extract_column_rows(col: &ColumnData, row_indices: &[usize]) -> ColumnData {
    match col {
        ColumnData::Int(data) => {
            ColumnData::Int(row_indices.iter().map(|&i| data[i]).collect())
        }
        ColumnData::Long(data) => {
            ColumnData::Long(row_indices.iter().map(|&i| data[i]).collect())
        }
        ColumnData::Float(data) => {
            ColumnData::Float(row_indices.iter().map(|&i| data[i]).collect())
        }
        ColumnData::Double(data) => {
            ColumnData::Double(row_indices.iter().map(|&i| data[i]).collect())
        }
        ColumnData::String(data) => {
            ColumnData::String(row_indices.iter().map(|&i| data[i].clone()).collect())
        }
        ColumnData::Bytes(data) => {
            ColumnData::Bytes(row_indices.iter().map(|&i| data[i].clone()).collect())
        }
        ColumnData::Boolean(data) => {
            ColumnData::Boolean(row_indices.iter().map(|&i| data[i]).collect())
        }
        ColumnData::Nulls(data) => {
            ColumnData::Nulls(row_indices.iter().map(|&i| data[i]).collect())
        }
    }
}

/// Builder for BlockExchange
pub struct BlockExchangeBuilder {
    exchange_type: ExchangeType,
    mailboxes: Vec<Arc<SendingMailbox>>,
    hash_key_columns: Vec<usize>,
    timeout: Duration,
    max_block_size: usize,
}

impl BlockExchangeBuilder {
    pub fn new(exchange_type: ExchangeType) -> Self {
        Self {
            exchange_type,
            mailboxes: Vec::new(),
            hash_key_columns: Vec::new(),
            timeout: Duration::from_secs(30),
            max_block_size: MAX_BLOCK_SIZE_BYTES,
        }
    }

    pub fn add_mailbox(mut self, mailbox: Arc<SendingMailbox>) -> Self {
        self.mailboxes.push(mailbox);
        self
    }

    pub fn with_mailboxes(mut self, mailboxes: Vec<Arc<SendingMailbox>>) -> Self {
        self.mailboxes = mailboxes;
        self
    }

    pub fn with_hash_keys(mut self, columns: Vec<usize>) -> Self {
        self.hash_key_columns = columns;
        self
    }

    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    pub fn with_max_block_size(mut self, size: usize) -> Self {
        self.max_block_size = size;
        self
    }

    pub fn build(self) -> BlockExchange {
        BlockExchange::new(
            self.exchange_type,
            self.mailboxes,
            self.hash_key_columns,
            self.timeout,
        )
        .with_max_block_size(self.max_block_size)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::block::{BlockId, BlockSchema, ColumnSchema, ColumnType, DataBlockBuilder};
    use crate::mailbox::{MailboxId, ReceivingMailbox};
    use std::sync::Arc;

    fn create_test_schema() -> Arc<BlockSchema> {
        Arc::new(BlockSchema::new(vec![
            ColumnSchema {
                name: "id".to_string(),
                data_type: ColumnType::Int,
                nullable: false,
            },
            ColumnSchema {
                name: "name".to_string(),
                data_type: ColumnType::String,
                nullable: false,
            },
        ]))
    }

    fn create_test_mailboxes(count: usize) -> (Vec<Arc<SendingMailbox>>, Vec<Arc<ReceivingMailbox>>) {
        let mut senders = Vec::new();
        let mut receivers = Vec::new();

        for i in 0..count {
            let id = MailboxId::new("q1", 0, 0, 1, i as i32);
            let receiver = Arc::new(ReceivingMailbox::new(id.clone(), 10));
            let sender = Arc::new(SendingMailbox::new_local(id, receiver.clone()));
            receivers.push(receiver);
            senders.push(sender);
        }

        (senders, receivers)
    }

    #[test]
    fn test_exchange_type_display() {
        assert_eq!(ExchangeType::Singleton.to_string(), "SINGLETON");
        assert_eq!(ExchangeType::HashDistributed.to_string(), "HASH");
    }

    #[test]
    fn test_singleton_exchange() {
        let (senders, receivers) = create_test_mailboxes(1);
        let exchange = BlockExchangeBuilder::new(ExchangeType::Singleton)
            .with_mailboxes(senders)
            .build();

        let schema = create_test_schema();
        let id = BlockId::new("q1", 0, 0, 1);
        let block = DataBlockBuilder::new(id, schema)
            .add_int_column(vec![1, 2, 3])
            .add_string_column(vec!["a".to_string(), "b".to_string(), "c".to_string()])
            .build();

        exchange.send(MseBlock::Data(block)).unwrap();

        let received = receivers[0].receive(Duration::from_secs(1)).unwrap();
        assert!(received.is_some());
    }

    #[test]
    fn test_broadcast_exchange() {
        let (senders, receivers) = create_test_mailboxes(3);
        let exchange = BlockExchangeBuilder::new(ExchangeType::BroadcastDistributed)
            .with_mailboxes(senders)
            .build();

        let schema = create_test_schema();
        let id = BlockId::new("q1", 0, 0, 1);
        let block = DataBlockBuilder::new(id, schema)
            .add_int_column(vec![1, 2, 3])
            .add_string_column(vec!["a".to_string(), "b".to_string(), "c".to_string()])
            .build();

        exchange.send(MseBlock::Data(block)).unwrap();

        // All receivers should get the block
        for receiver in &receivers {
            let received = receiver.receive(Duration::from_secs(1)).unwrap();
            assert!(received.is_some());
        }
    }

    #[test]
    fn test_hash_distributed_exchange() {
        let (senders, receivers) = create_test_mailboxes(2);
        let exchange = BlockExchangeBuilder::new(ExchangeType::HashDistributed)
            .with_mailboxes(senders)
            .with_hash_keys(vec![0]) // Hash on first column
            .build();

        let schema = create_test_schema();
        let id = BlockId::new("q1", 0, 0, 1);
        let block = DataBlockBuilder::new(id, schema)
            .add_int_column(vec![1, 2, 3, 4, 5, 6])
            .add_string_column(vec![
                "a".to_string(),
                "b".to_string(),
                "c".to_string(),
                "d".to_string(),
                "e".to_string(),
                "f".to_string(),
            ])
            .build();

        exchange.send(MseBlock::Data(block)).unwrap();

        // Both receivers should get some data
        let mut total_rows = 0;
        for receiver in &receivers {
            while let Some(block) = receiver.poll() {
                if let MseBlock::Data(db) = block {
                    total_rows += db.num_rows;
                }
            }
        }

        assert_eq!(total_rows, 6);
    }

    #[test]
    fn test_random_exchange() {
        let (senders, receivers) = create_test_mailboxes(3);
        let exchange = BlockExchangeBuilder::new(ExchangeType::RandomDistributed)
            .with_mailboxes(senders)
            .build();

        let schema = create_test_schema();

        // Send multiple blocks
        for i in 0..6 {
            let id = BlockId::new("q1", 0, 0, i);
            let block = DataBlockBuilder::new(id, schema.clone())
                .add_int_column(vec![i as i32])
                .add_string_column(vec![format!("val_{}", i)])
                .build();
            exchange.send(MseBlock::Data(block)).unwrap();
        }

        // Count blocks across all receivers
        let mut total_blocks = 0;
        for receiver in &receivers {
            while receiver.poll().is_some() {
                total_blocks += 1;
            }
        }

        assert_eq!(total_blocks, 6);
    }

    #[test]
    fn test_exchange_stats() {
        let (senders, _receivers) = create_test_mailboxes(1);
        let exchange = BlockExchangeBuilder::new(ExchangeType::Singleton)
            .with_mailboxes(senders)
            .build();

        let schema = create_test_schema();
        let id = BlockId::new("q1", 0, 0, 1);
        let block = DataBlockBuilder::new(id, schema)
            .add_int_column(vec![1, 2, 3])
            .add_string_column(vec!["a".to_string(), "b".to_string(), "c".to_string()])
            .build();

        exchange.send(MseBlock::Data(block)).unwrap();

        let stats = exchange.stats();
        assert_eq!(stats.blocks_sent.load(Ordering::Relaxed), 1);
        assert_eq!(stats.rows_sent.load(Ordering::Relaxed), 3);
    }
}
