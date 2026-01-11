//! Query operators for multi-stage execution
//!
//! This module provides composable operators for building query execution pipelines.
//! Operators consume and produce data blocks, forming execution chains.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use async_trait::async_trait;
use parking_lot::Mutex;

use crate::backpressure::{MemoryBackpressureManager, OperatorBackpressure};
use crate::block::{BlockStats, DataBlock, MseBlock};
use crate::error::{QueryError, Result};
use crate::exchange::BlockExchange;
use crate::mailbox::{BlockingMultiStreamConsumer, ReceivingMailbox, SendingMailbox};

/// Operator trait for query execution
#[async_trait]
pub trait Operator: Send + Sync {
    /// Get operator name
    fn name(&self) -> &str;

    /// Get next block from this operator
    async fn next_block(&self) -> Result<Option<MseBlock>>;

    /// Check if operator has more data
    fn has_next(&self) -> bool;

    /// Get operator statistics
    fn stats(&self) -> BlockStats;

    /// Close the operator
    fn close(&self);

    /// Check if operator is closed
    fn is_closed(&self) -> bool;

    /// Early terminate the operator
    fn early_terminate(&self);
}

/// Base operator state
pub struct OperatorState {
    /// Operator name
    name: String,
    /// Query ID
    query_id: String,
    /// Stage ID
    stage_id: i32,
    /// Worker ID
    worker_id: i32,
    /// Closed flag
    closed: AtomicBool,
    /// Early terminated flag
    early_terminated: AtomicBool,
    /// Statistics
    stats: Mutex<BlockStats>,
    /// Start time
    start_time: Instant,
}

impl OperatorState {
    pub fn new(name: impl Into<String>, query_id: impl Into<String>, stage_id: i32, worker_id: i32) -> Self {
        Self {
            name: name.into(),
            query_id: query_id.into(),
            stage_id,
            worker_id,
            closed: AtomicBool::new(false),
            early_terminated: AtomicBool::new(false),
            stats: Mutex::new(BlockStats::default()),
            start_time: Instant::now(),
        }
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn is_closed(&self) -> bool {
        self.closed.load(Ordering::Relaxed)
    }

    pub fn close(&self) {
        self.closed.store(true, Ordering::Relaxed);
    }

    pub fn is_early_terminated(&self) -> bool {
        self.early_terminated.load(Ordering::Relaxed)
    }

    pub fn early_terminate(&self) {
        self.early_terminated.store(true, Ordering::Relaxed);
    }

    pub fn record_rows(&self, rows: u64) {
        self.stats.lock().emitted_rows += rows;
    }

    pub fn record_bytes(&self, bytes: u64) {
        self.stats.lock().serialized_bytes += bytes;
    }

    pub fn record_wait(&self, wait_ms: u64) {
        self.stats.lock().downstream_wait_ms += wait_ms;
    }

    pub fn stats(&self) -> BlockStats {
        let mut stats = self.stats.lock().clone();
        stats.execution_time_ms = self.start_time.elapsed().as_millis() as u64;
        stats
    }
}

/// Mailbox receive operator
pub struct MailboxReceiveOperator {
    state: OperatorState,
    consumer: BlockingMultiStreamConsumer,
    timeout: Duration,
}

impl MailboxReceiveOperator {
    pub fn new(
        query_id: impl Into<String>,
        stage_id: i32,
        worker_id: i32,
        mailboxes: Vec<Arc<ReceivingMailbox>>,
        timeout: Duration,
    ) -> Self {
        let consumer = BlockingMultiStreamConsumer::new(mailboxes);
        Self {
            state: OperatorState::new("MailboxReceive", query_id, stage_id, worker_id),
            consumer,
            timeout,
        }
    }

    pub fn active_streams(&self) -> usize {
        self.consumer.active_count()
    }
}

#[async_trait]
impl Operator for MailboxReceiveOperator {
    fn name(&self) -> &str {
        self.state.name()
    }

    async fn next_block(&self) -> Result<Option<MseBlock>> {
        if self.state.is_closed() || self.state.is_early_terminated() {
            return Ok(None);
        }

        let start = Instant::now();
        let result = self.consumer.read_async(self.timeout).await?;

        if let Some(ref block) = result {
            if let MseBlock::Data(data) = block {
                self.state.record_rows(data.num_rows as u64);
                self.state.record_bytes(data.memory_size() as u64);
            }
        }

        let wait_time = start.elapsed().as_millis() as u64;
        if wait_time > 1 {
            self.state.record_wait(wait_time);
        }

        Ok(result)
    }

    fn has_next(&self) -> bool {
        !self.state.is_closed() && self.consumer.active_count() > 0
    }

    fn stats(&self) -> BlockStats {
        self.state.stats()
    }

    fn close(&self) {
        self.state.close();
    }

    fn is_closed(&self) -> bool {
        self.state.is_closed()
    }

    fn early_terminate(&self) {
        self.state.early_terminate();
    }
}

/// Mailbox send operator
pub struct MailboxSendOperator {
    state: OperatorState,
    upstream: Arc<dyn Operator>,
    exchange: BlockExchange,
    blocks_sent: AtomicU64,
}

impl MailboxSendOperator {
    pub fn new(
        query_id: impl Into<String>,
        stage_id: i32,
        worker_id: i32,
        upstream: Arc<dyn Operator>,
        exchange: BlockExchange,
    ) -> Self {
        Self {
            state: OperatorState::new("MailboxSend", query_id, stage_id, worker_id),
            upstream,
            exchange,
            blocks_sent: AtomicU64::new(0),
        }
    }

    /// Run the send operator until completion
    pub async fn run(&self) -> Result<BlockStats> {
        while let Some(block) = self.upstream.next_block().await? {
            if self.state.is_early_terminated() {
                break;
            }

            // Check for early termination from downstream
            if self.exchange.check_early_termination() {
                self.upstream.early_terminate();
                break;
            }

            if let MseBlock::Data(ref data) = block {
                self.state.record_rows(data.num_rows as u64);
            }

            self.exchange.send(block)?;
            self.blocks_sent.fetch_add(1, Ordering::Relaxed);
        }

        // Send EOS
        let eos = MseBlock::Eos {
            query_id: self.state.query_id.clone(),
            stage_id: self.state.stage_id,
            stats: Some(self.state.stats()),
        };
        self.exchange.send(eos)?;

        Ok(self.state.stats())
    }

    pub fn blocks_sent(&self) -> u64 {
        self.blocks_sent.load(Ordering::Relaxed)
    }
}

#[async_trait]
impl Operator for MailboxSendOperator {
    fn name(&self) -> &str {
        self.state.name()
    }

    async fn next_block(&self) -> Result<Option<MseBlock>> {
        // Send operator doesn't produce blocks, it sends them
        Ok(None)
    }

    fn has_next(&self) -> bool {
        false
    }

    fn stats(&self) -> BlockStats {
        self.state.stats()
    }

    fn close(&self) {
        self.exchange.close();
        self.state.close();
    }

    fn is_closed(&self) -> bool {
        self.state.is_closed()
    }

    fn early_terminate(&self) {
        self.upstream.early_terminate();
        self.state.early_terminate();
    }
}

/// Filter operator
pub struct FilterOperator {
    state: OperatorState,
    upstream: Arc<dyn Operator>,
    predicate: Box<dyn Fn(&DataBlock, usize) -> bool + Send + Sync>,
}

impl FilterOperator {
    pub fn new<F>(
        query_id: impl Into<String>,
        stage_id: i32,
        worker_id: i32,
        upstream: Arc<dyn Operator>,
        predicate: F,
    ) -> Self
    where
        F: Fn(&DataBlock, usize) -> bool + Send + Sync + 'static,
    {
        Self {
            state: OperatorState::new("Filter", query_id, stage_id, worker_id),
            upstream,
            predicate: Box::new(predicate),
        }
    }
}

#[async_trait]
impl Operator for FilterOperator {
    fn name(&self) -> &str {
        self.state.name()
    }

    async fn next_block(&self) -> Result<Option<MseBlock>> {
        if self.state.is_closed() {
            return Ok(None);
        }

        while let Some(block) = self.upstream.next_block().await? {
            match block {
                MseBlock::Data(data) => {
                    // Filter rows
                    let mut keep_indices = Vec::new();
                    for i in 0..data.num_rows {
                        if (self.predicate)(&data, i) {
                            keep_indices.push(i);
                        }
                    }

                    if !keep_indices.is_empty() {
                        // Build filtered block
                        let filtered_columns: Vec<_> = data
                            .columns
                            .iter()
                            .map(|col| extract_column_rows(col, &keep_indices))
                            .collect();

                        let filtered = DataBlock::new(data.id.clone(), data.schema.clone(), filtered_columns);
                        self.state.record_rows(filtered.num_rows as u64);
                        return Ok(Some(MseBlock::Data(filtered)));
                    }
                    // Continue to next block if nothing matched
                }
                other => return Ok(Some(other)),
            }
        }

        Ok(None)
    }

    fn has_next(&self) -> bool {
        !self.state.is_closed() && self.upstream.has_next()
    }

    fn stats(&self) -> BlockStats {
        self.state.stats()
    }

    fn close(&self) {
        self.upstream.close();
        self.state.close();
    }

    fn is_closed(&self) -> bool {
        self.state.is_closed()
    }

    fn early_terminate(&self) {
        self.upstream.early_terminate();
        self.state.early_terminate();
    }
}

/// Extract rows from a column
fn extract_column_rows(col: &crate::block::ColumnData, row_indices: &[usize]) -> crate::block::ColumnData {
    use crate::block::ColumnData;
    match col {
        ColumnData::Int(data) => ColumnData::Int(row_indices.iter().map(|&i| data[i]).collect()),
        ColumnData::Long(data) => ColumnData::Long(row_indices.iter().map(|&i| data[i]).collect()),
        ColumnData::Float(data) => ColumnData::Float(row_indices.iter().map(|&i| data[i]).collect()),
        ColumnData::Double(data) => ColumnData::Double(row_indices.iter().map(|&i| data[i]).collect()),
        ColumnData::String(data) => ColumnData::String(row_indices.iter().map(|&i| data[i].clone()).collect()),
        ColumnData::Bytes(data) => ColumnData::Bytes(row_indices.iter().map(|&i| data[i].clone()).collect()),
        ColumnData::Boolean(data) => ColumnData::Boolean(row_indices.iter().map(|&i| data[i]).collect()),
        ColumnData::Nulls(data) => ColumnData::Nulls(row_indices.iter().map(|&i| data[i]).collect()),
    }
}

/// Limit operator
pub struct LimitOperator {
    state: OperatorState,
    upstream: Arc<dyn Operator>,
    limit: usize,
    rows_emitted: AtomicU64,
}

impl LimitOperator {
    pub fn new(
        query_id: impl Into<String>,
        stage_id: i32,
        worker_id: i32,
        upstream: Arc<dyn Operator>,
        limit: usize,
    ) -> Self {
        Self {
            state: OperatorState::new("Limit", query_id, stage_id, worker_id),
            upstream,
            limit,
            rows_emitted: AtomicU64::new(0),
        }
    }
}

#[async_trait]
impl Operator for LimitOperator {
    fn name(&self) -> &str {
        self.state.name()
    }

    async fn next_block(&self) -> Result<Option<MseBlock>> {
        let emitted = self.rows_emitted.load(Ordering::Relaxed);
        if emitted >= self.limit as u64 || self.state.is_closed() {
            return Ok(None);
        }

        while let Some(block) = self.upstream.next_block().await? {
            match block {
                MseBlock::Data(data) => {
                    let current = self.rows_emitted.load(Ordering::Relaxed) as usize;
                    let remaining = self.limit.saturating_sub(current);

                    if remaining == 0 {
                        self.upstream.early_terminate();
                        return Ok(None);
                    }

                    if data.num_rows <= remaining {
                        self.rows_emitted.fetch_add(data.num_rows as u64, Ordering::Relaxed);
                        self.state.record_rows(data.num_rows as u64);
                        return Ok(Some(MseBlock::Data(data)));
                    }

                    // Take only remaining rows
                    let keep_indices: Vec<_> = (0..remaining).collect();
                    let limited_columns: Vec<_> = data
                        .columns
                        .iter()
                        .map(|col| extract_column_rows(col, &keep_indices))
                        .collect();

                    let limited = DataBlock::new(data.id.clone(), data.schema.clone(), limited_columns);
                    self.rows_emitted.fetch_add(limited.num_rows as u64, Ordering::Relaxed);
                    self.state.record_rows(limited.num_rows as u64);
                    self.upstream.early_terminate();
                    return Ok(Some(MseBlock::Data(limited)));
                }
                other => return Ok(Some(other)),
            }
        }

        Ok(None)
    }

    fn has_next(&self) -> bool {
        let emitted = self.rows_emitted.load(Ordering::Relaxed);
        !self.state.is_closed() && emitted < self.limit as u64 && self.upstream.has_next()
    }

    fn stats(&self) -> BlockStats {
        self.state.stats()
    }

    fn close(&self) {
        self.upstream.close();
        self.state.close();
    }

    fn is_closed(&self) -> bool {
        self.state.is_closed()
    }

    fn early_terminate(&self) {
        self.upstream.early_terminate();
        self.state.early_terminate();
    }
}

/// Operator chain for sequential execution
pub struct OpChain {
    /// Chain ID
    id: String,
    /// Root operator
    root: Arc<dyn Operator>,
    /// Backpressure controller
    backpressure: Option<OperatorBackpressure>,
    /// Statistics
    stats: Mutex<BlockStats>,
    /// Start time
    start_time: Instant,
    /// Closed flag
    closed: AtomicBool,
}

impl OpChain {
    /// Create a new operator chain
    pub fn new(id: impl Into<String>, root: Arc<dyn Operator>) -> Self {
        Self {
            id: id.into(),
            root,
            backpressure: None,
            stats: Mutex::new(BlockStats::default()),
            start_time: Instant::now(),
            closed: AtomicBool::new(false),
        }
    }

    /// Add backpressure controller
    pub fn with_backpressure(mut self, manager: Arc<MemoryBackpressureManager>) -> Self {
        self.backpressure = Some(OperatorBackpressure::new(manager, &self.id));
        self
    }

    /// Get chain ID
    pub fn id(&self) -> &str {
        &self.id
    }

    /// Execute the operator chain
    pub async fn execute(&self) -> Result<Vec<DataBlock>> {
        let mut results = Vec::new();

        while let Some(block) = self.root.next_block().await? {
            if self.closed.load(Ordering::Relaxed) {
                break;
            }

            if let Some(ref bp) = self.backpressure {
                if bp.should_stop() {
                    self.root.early_terminate();
                    break;
                }
            }

            match block {
                MseBlock::Data(data) => {
                    let rows = data.num_rows as u64;
                    let bytes = data.memory_size();

                    if let Some(ref bp) = self.backpressure {
                        bp.record_and_check(bytes, rows as usize).await;
                    }

                    self.stats.lock().emitted_rows += rows;
                    results.push(data);
                }
                MseBlock::Eos { .. } => break,
                MseBlock::Error { error_message, .. } => {
                    return Err(QueryError::Operator {
                        operator: self.root.name().to_string(),
                        message: error_message,
                    });
                }
                MseBlock::Success { .. } => break,
            }
        }

        self.stats.lock().execution_time_ms = self.start_time.elapsed().as_millis() as u64;
        Ok(results)
    }

    /// Get statistics
    pub fn stats(&self) -> BlockStats {
        self.stats.lock().clone()
    }

    /// Close the chain
    pub fn close(&self) {
        self.closed.store(true, Ordering::Relaxed);
        self.root.close();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::block::{BlockId, BlockSchema, ColumnData, ColumnSchema, ColumnType, DataBlockBuilder};
    use crate::mailbox::MailboxId;
    use std::sync::Arc;

    fn create_test_schema() -> Arc<BlockSchema> {
        Arc::new(BlockSchema::new(vec![
            ColumnSchema {
                name: "id".to_string(),
                data_type: ColumnType::Int,
                nullable: false,
            },
        ]))
    }

    struct MockOperator {
        blocks: Mutex<Vec<MseBlock>>,
        state: OperatorState,
    }

    impl MockOperator {
        fn new(blocks: Vec<MseBlock>) -> Self {
            Self {
                blocks: Mutex::new(blocks),
                state: OperatorState::new("Mock", "q1", 0, 0),
            }
        }
    }

    #[async_trait]
    impl Operator for MockOperator {
        fn name(&self) -> &str {
            "Mock"
        }

        async fn next_block(&self) -> Result<Option<MseBlock>> {
            let mut blocks = self.blocks.lock();
            if blocks.is_empty() {
                Ok(None)
            } else {
                Ok(Some(blocks.remove(0)))
            }
        }

        fn has_next(&self) -> bool {
            !self.blocks.lock().is_empty()
        }

        fn stats(&self) -> BlockStats {
            BlockStats::default()
        }

        fn close(&self) {
            self.state.close();
        }

        fn is_closed(&self) -> bool {
            self.state.is_closed()
        }

        fn early_terminate(&self) {
            self.state.early_terminate();
        }
    }

    #[tokio::test]
    async fn test_limit_operator() {
        let schema = create_test_schema();

        let blocks: Vec<MseBlock> = (0..5)
            .map(|i| {
                let id = BlockId::new("q1", 0, 0, i);
                let block = DataBlockBuilder::new(id, schema.clone())
                    .add_int_column(vec![1, 2, 3]) // 3 rows each
                    .build();
                MseBlock::Data(block)
            })
            .collect();

        let mock = Arc::new(MockOperator::new(blocks));
        let limit = LimitOperator::new("q1", 0, 0, mock, 5);

        let mut total_rows = 0;
        while let Some(block) = limit.next_block().await.unwrap() {
            if let MseBlock::Data(data) = block {
                total_rows += data.num_rows;
            }
        }

        assert_eq!(total_rows, 5);
    }

    #[tokio::test]
    async fn test_filter_operator() {
        let schema = create_test_schema();
        let id = BlockId::new("q1", 0, 0, 1);
        let block = DataBlockBuilder::new(id, schema.clone())
            .add_int_column(vec![1, 2, 3, 4, 5])
            .build();

        let mock = Arc::new(MockOperator::new(vec![MseBlock::Data(block)]));

        // Filter for values > 3
        let filter = FilterOperator::new("q1", 0, 0, mock, |block, row| {
            if let Some(ColumnData::Int(data)) = block.columns.first() {
                data[row] > 3
            } else {
                false
            }
        });

        let result = filter.next_block().await.unwrap();
        assert!(result.is_some());

        if let Some(MseBlock::Data(data)) = result {
            assert_eq!(data.num_rows, 2); // 4 and 5
        }
    }

    #[tokio::test]
    async fn test_op_chain() {
        let schema = create_test_schema();
        let blocks: Vec<MseBlock> = (0..3)
            .map(|i| {
                let id = BlockId::new("q1", 0, 0, i);
                let block = DataBlockBuilder::new(id, schema.clone())
                    .add_int_column(vec![1, 2])
                    .build();
                MseBlock::Data(block)
            })
            .collect();

        let mock = Arc::new(MockOperator::new(blocks));
        let chain = OpChain::new("chain-1", mock);

        let results = chain.execute().await.unwrap();
        assert_eq!(results.len(), 3);
    }
}
