//! Mailbox system for inter-process communication
//!
//! The mailbox system provides bounded async channels with backpressure support
//! for exchanging data blocks between query stages.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use crossbeam_channel::{bounded, Receiver, Sender, TrySendError};
use dashmap::DashMap;
use parking_lot::{Mutex, RwLock};
use tokio::sync::Notify;

use crate::block::{BlockStats, MseBlock};
use crate::config::QueryServerConfig;
use crate::error::{QueryError, Result};
use crate::{DEFAULT_MAILBOX_EXPIRY_SECONDS, DEFAULT_MAX_PENDING_BLOCKS};

/// Unique identifier for a mailbox
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct MailboxId {
    /// Query ID
    pub query_id: String,
    /// Sending stage ID
    pub sender_stage_id: i32,
    /// Sending worker ID
    pub sender_worker_id: i32,
    /// Receiving stage ID
    pub receiver_stage_id: i32,
    /// Receiving worker ID
    pub receiver_worker_id: i32,
}

impl MailboxId {
    pub fn new(
        query_id: impl Into<String>,
        sender_stage_id: i32,
        sender_worker_id: i32,
        receiver_stage_id: i32,
        receiver_worker_id: i32,
    ) -> Self {
        Self {
            query_id: query_id.into(),
            sender_stage_id,
            sender_worker_id,
            receiver_stage_id,
            receiver_worker_id,
        }
    }

    /// Create ID string for logging
    pub fn to_string(&self) -> String {
        format!(
            "{}:{}:{}->{}:{}",
            self.query_id,
            self.sender_stage_id,
            self.sender_worker_id,
            self.receiver_stage_id,
            self.receiver_worker_id
        )
    }
}

impl std::fmt::Display for MailboxId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.to_string())
    }
}

/// State of a receiving mailbox
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MailboxState {
    /// Mailbox is open and accepting data
    Open,
    /// Upstream has finished sending
    UpstreamFinished,
    /// Waiting for end-of-stream marker
    WaitingEos,
    /// Mailbox is fully closed
    Closed,
    /// Mailbox was cancelled
    Cancelled,
}

/// Statistics for mailbox operations
#[derive(Debug, Default)]
pub struct MailboxStats {
    /// Number of blocks received
    pub blocks_received: AtomicU64,
    /// Number of blocks sent
    pub blocks_sent: AtomicU64,
    /// Total bytes received
    pub bytes_received: AtomicU64,
    /// Total bytes sent
    pub bytes_sent: AtomicU64,
    /// Number of backpressure events (queue full)
    pub backpressure_events: AtomicU64,
    /// Total wait time due to backpressure (microseconds)
    pub backpressure_wait_us: AtomicU64,
    /// Number of early termination signals
    pub early_terminations: AtomicU64,
}

impl MailboxStats {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn record_receive(&self, bytes: usize) {
        self.blocks_received.fetch_add(1, Ordering::Relaxed);
        self.bytes_received.fetch_add(bytes as u64, Ordering::Relaxed);
    }

    pub fn record_send(&self, bytes: usize) {
        self.blocks_sent.fetch_add(1, Ordering::Relaxed);
        self.bytes_sent.fetch_add(bytes as u64, Ordering::Relaxed);
    }

    pub fn record_backpressure(&self, wait_us: u64) {
        self.backpressure_events.fetch_add(1, Ordering::Relaxed);
        self.backpressure_wait_us.fetch_add(wait_us, Ordering::Relaxed);
    }

    pub fn to_block_stats(&self) -> BlockStats {
        BlockStats {
            emitted_rows: self.blocks_sent.load(Ordering::Relaxed),
            serialized_bytes: self.bytes_sent.load(Ordering::Relaxed),
            deserialized_bytes: self.bytes_received.load(Ordering::Relaxed),
            downstream_wait_ms: self.backpressure_wait_us.load(Ordering::Relaxed) / 1000,
            ..Default::default()
        }
    }
}

/// Receiving mailbox for collecting data from senders
pub struct ReceivingMailbox {
    /// Mailbox identifier
    id: MailboxId,
    /// Bounded channel receiver
    receiver: Receiver<MseBlock>,
    /// Current state
    state: RwLock<MailboxState>,
    /// Number of pending blocks
    pending_blocks: AtomicUsize,
    /// Maximum pending blocks (backpressure threshold)
    max_pending_blocks: usize,
    /// Statistics
    stats: Arc<MailboxStats>,
    /// Notification for new data
    notify: Arc<Notify>,
    /// Early termination flag
    early_terminated: AtomicBool,
    /// Creation time
    created_at: Instant,
    /// Last activity time
    last_activity: RwLock<Instant>,
    /// Associated sender for backpressure feedback
    sender: Sender<MseBlock>,
}

impl ReceivingMailbox {
    /// Create a new receiving mailbox
    pub fn new(id: MailboxId, max_pending_blocks: usize) -> Self {
        let (sender, receiver) = bounded(max_pending_blocks);
        let now = Instant::now();

        Self {
            id,
            receiver,
            state: RwLock::new(MailboxState::Open),
            pending_blocks: AtomicUsize::new(0),
            max_pending_blocks,
            stats: Arc::new(MailboxStats::new()),
            notify: Arc::new(Notify::new()),
            early_terminated: AtomicBool::new(false),
            created_at: now,
            last_activity: RwLock::new(now),
            sender,
        }
    }

    /// Get mailbox ID
    pub fn id(&self) -> &MailboxId {
        &self.id
    }

    /// Get current state
    pub fn state(&self) -> MailboxState {
        *self.state.read()
    }

    /// Get number of pending blocks
    pub fn pending_blocks(&self) -> usize {
        self.pending_blocks.load(Ordering::Relaxed)
    }

    /// Check if mailbox is full (at backpressure threshold)
    pub fn is_full(&self) -> bool {
        self.pending_blocks() >= self.max_pending_blocks
    }

    /// Check if early termination was signaled
    pub fn is_early_terminated(&self) -> bool {
        self.early_terminated.load(Ordering::Relaxed)
    }

    /// Signal early termination
    pub fn signal_early_termination(&self) {
        self.early_terminated.store(true, Ordering::Relaxed);
        self.stats.early_terminations.fetch_add(1, Ordering::Relaxed);
    }

    /// Get statistics
    pub fn stats(&self) -> &MailboxStats {
        &self.stats
    }

    /// Get sender for offering blocks
    pub fn sender(&self) -> Sender<MseBlock> {
        self.sender.clone()
    }

    /// Offer a block to the mailbox with timeout
    pub fn offer(&self, block: MseBlock, timeout: Duration) -> Result<()> {
        // Check if early terminated
        if self.is_early_terminated() {
            return Err(QueryError::EarlyTermination(self.id.to_string()));
        }

        // Check state
        let state = *self.state.read();
        if state == MailboxState::Closed || state == MailboxState::Cancelled {
            return Err(QueryError::MailboxClosed(self.id.to_string()));
        }

        let bytes = block.memory_size();
        let start = Instant::now();

        // Try to send with timeout
        match self.sender.send_timeout(block, timeout) {
            Ok(()) => {
                self.pending_blocks.fetch_add(1, Ordering::Relaxed);
                self.stats.record_receive(bytes);
                *self.last_activity.write() = Instant::now();
                self.notify.notify_one();
                Ok(())
            }
            Err(crossbeam_channel::SendTimeoutError::Timeout(_)) => {
                let wait_us = start.elapsed().as_micros() as u64;
                self.stats.record_backpressure(wait_us);
                Err(QueryError::MailboxTimeout {
                    mailbox_id: self.id.to_string(),
                    timeout_ms: timeout.as_millis() as u64,
                })
            }
            Err(crossbeam_channel::SendTimeoutError::Disconnected(_)) => {
                Err(QueryError::MailboxClosed(self.id.to_string()))
            }
        }
    }

    /// Poll for a block (non-blocking)
    pub fn poll(&self) -> Option<MseBlock> {
        match self.receiver.try_recv() {
            Ok(block) => {
                self.pending_blocks.fetch_sub(1, Ordering::Relaxed);
                *self.last_activity.write() = Instant::now();
                self.update_state_on_receive(&block);
                Some(block)
            }
            Err(_) => None,
        }
    }

    /// Receive a block (blocking with timeout)
    pub fn receive(&self, timeout: Duration) -> Result<Option<MseBlock>> {
        match self.receiver.recv_timeout(timeout) {
            Ok(block) => {
                self.pending_blocks.fetch_sub(1, Ordering::Relaxed);
                *self.last_activity.write() = Instant::now();
                self.update_state_on_receive(&block);
                Ok(Some(block))
            }
            Err(crossbeam_channel::RecvTimeoutError::Timeout) => Ok(None),
            Err(crossbeam_channel::RecvTimeoutError::Disconnected) => {
                *self.state.write() = MailboxState::Closed;
                Ok(None)
            }
        }
    }

    /// Async receive using notification
    pub async fn receive_async(&self, timeout: Duration) -> Result<Option<MseBlock>> {
        // First try non-blocking
        if let Some(block) = self.poll() {
            return Ok(Some(block));
        }

        // Wait for notification with timeout
        let notify = self.notify.clone();
        let result = tokio::time::timeout(timeout, notify.notified()).await;

        if result.is_ok() {
            Ok(self.poll())
        } else {
            Ok(None)
        }
    }

    /// Update state based on received block
    fn update_state_on_receive(&self, block: &MseBlock) {
        if block.is_eos() {
            *self.state.write() = MailboxState::Closed;
        }
    }

    /// Mark mailbox as closed
    pub fn close(&self) {
        *self.state.write() = MailboxState::Closed;
    }

    /// Cancel the mailbox
    pub fn cancel(&self) {
        *self.state.write() = MailboxState::Cancelled;
        self.signal_early_termination();
    }

    /// Drain remaining blocks
    pub fn drain(&self) -> Vec<MseBlock> {
        let mut blocks = Vec::new();
        while let Some(block) = self.poll() {
            blocks.push(block);
        }
        blocks
    }

    /// Check if mailbox has expired
    pub fn is_expired(&self, expiry: Duration) -> bool {
        self.last_activity.read().elapsed() > expiry
    }
}

/// Sending mailbox for transmitting data to receivers
pub struct SendingMailbox {
    /// Mailbox identifier
    id: MailboxId,
    /// Target receiving mailbox (for local sends)
    local_receiver: Option<Arc<ReceivingMailbox>>,
    /// Remote target address (for network sends)
    remote_target: Option<(String, u16)>,
    /// Statistics
    stats: Arc<MailboxStats>,
    /// Closed flag
    closed: AtomicBool,
    /// Early termination received
    early_terminated: AtomicBool,
}

impl SendingMailbox {
    /// Create a local sending mailbox (same process)
    pub fn new_local(id: MailboxId, receiver: Arc<ReceivingMailbox>) -> Self {
        Self {
            id,
            local_receiver: Some(receiver),
            remote_target: None,
            stats: Arc::new(MailboxStats::new()),
            closed: AtomicBool::new(false),
            early_terminated: AtomicBool::new(false),
        }
    }

    /// Create a remote sending mailbox (network)
    pub fn new_remote(id: MailboxId, host: String, port: u16) -> Self {
        Self {
            id,
            local_receiver: None,
            remote_target: Some((host, port)),
            stats: Arc::new(MailboxStats::new()),
            closed: AtomicBool::new(false),
            early_terminated: AtomicBool::new(false),
        }
    }

    /// Get mailbox ID
    pub fn id(&self) -> &MailboxId {
        &self.id
    }

    /// Check if this is a local mailbox
    pub fn is_local(&self) -> bool {
        self.local_receiver.is_some()
    }

    /// Check if closed
    pub fn is_closed(&self) -> bool {
        self.closed.load(Ordering::Relaxed)
    }

    /// Check if early termination was signaled
    pub fn is_early_terminated(&self) -> bool {
        self.early_terminated.load(Ordering::Relaxed)
    }

    /// Receive early termination signal
    pub fn receive_early_termination(&self) {
        self.early_terminated.store(true, Ordering::Relaxed);
    }

    /// Send a block
    pub fn send(&self, block: MseBlock, timeout: Duration) -> Result<()> {
        if self.is_closed() {
            return Err(QueryError::MailboxClosed(self.id.to_string()));
        }

        if self.is_early_terminated() {
            return Err(QueryError::EarlyTermination(self.id.to_string()));
        }

        let bytes = block.memory_size();

        if let Some(receiver) = &self.local_receiver {
            // Local send - direct to receiver
            receiver.offer(block, timeout)?;
        } else {
            // Remote send - handled by channel manager
            // For now, return not implemented
            return Err(QueryError::Internal(
                "Remote send not implemented in SendingMailbox directly".to_string(),
            ));
        }

        self.stats.record_send(bytes);
        Ok(())
    }

    /// Get statistics
    pub fn stats(&self) -> &MailboxStats {
        &self.stats
    }

    /// Close the mailbox
    pub fn close(&self) {
        self.closed.store(true, Ordering::Relaxed);
    }
}

/// Mailbox service managing all mailboxes
pub struct MailboxService {
    /// Configuration
    config: Arc<QueryServerConfig>,
    /// Receiving mailboxes by ID
    receiving_mailboxes: DashMap<String, Arc<ReceivingMailbox>>,
    /// Sending mailboxes by ID
    sending_mailboxes: DashMap<String, Arc<SendingMailbox>>,
    /// Expiry duration
    expiry: Duration,
    /// Service shutdown flag
    shutdown: AtomicBool,
}

impl MailboxService {
    /// Create a new mailbox service
    pub fn new(config: Arc<QueryServerConfig>) -> Self {
        let expiry = config.mailbox_expiry;
        Self {
            config,
            receiving_mailboxes: DashMap::new(),
            sending_mailboxes: DashMap::new(),
            expiry,
            shutdown: AtomicBool::new(false),
        }
    }

    /// Get or create a receiving mailbox
    pub fn get_or_create_receiving_mailbox(&self, id: MailboxId) -> Arc<ReceivingMailbox> {
        let key = id.to_string();
        self.receiving_mailboxes
            .entry(key)
            .or_insert_with(|| {
                Arc::new(ReceivingMailbox::new(
                    id,
                    self.config.max_pending_blocks,
                ))
            })
            .clone()
    }

    /// Get a receiving mailbox
    pub fn get_receiving_mailbox(&self, id: &MailboxId) -> Option<Arc<ReceivingMailbox>> {
        let key = id.to_string();
        self.receiving_mailboxes.get(&key).map(|r| r.clone())
    }

    /// Create a sending mailbox
    pub fn create_sending_mailbox(&self, id: MailboxId, is_local: bool, host: Option<String>, port: Option<u16>) -> Arc<SendingMailbox> {
        let key = id.to_string();

        let mailbox = if is_local {
            let receiver = self.get_or_create_receiving_mailbox(MailboxId::new(
                &id.query_id,
                id.sender_stage_id,
                id.sender_worker_id,
                id.receiver_stage_id,
                id.receiver_worker_id,
            ));
            Arc::new(SendingMailbox::new_local(id, receiver))
        } else {
            Arc::new(SendingMailbox::new_remote(
                id,
                host.unwrap_or_default(),
                port.unwrap_or(0),
            ))
        };

        self.sending_mailboxes.insert(key, mailbox.clone());
        mailbox
    }

    /// Get a sending mailbox
    pub fn get_sending_mailbox(&self, id: &MailboxId) -> Option<Arc<SendingMailbox>> {
        let key = id.to_string();
        self.sending_mailboxes.get(&key).map(|s| s.clone())
    }

    /// Remove mailboxes for a query
    pub fn remove_query_mailboxes(&self, query_id: &str) {
        self.receiving_mailboxes
            .retain(|k, _| !k.starts_with(query_id));
        self.sending_mailboxes
            .retain(|k, _| !k.starts_with(query_id));
    }

    /// Clean up expired mailboxes
    pub fn cleanup_expired(&self) {
        self.receiving_mailboxes.retain(|_, v| !v.is_expired(self.expiry));
    }

    /// Get total number of mailboxes
    pub fn mailbox_count(&self) -> (usize, usize) {
        (self.receiving_mailboxes.len(), self.sending_mailboxes.len())
    }

    /// Shutdown the service
    pub fn shutdown(&self) {
        self.shutdown.store(true, Ordering::Relaxed);
        // Cancel all mailboxes
        for entry in self.receiving_mailboxes.iter() {
            entry.value().cancel();
        }
        for entry in self.sending_mailboxes.iter() {
            entry.value().close();
        }
    }
}

/// Multi-stream consumer for merging blocks from multiple mailboxes
pub struct BlockingMultiStreamConsumer {
    /// Source mailboxes
    mailboxes: Vec<Arc<ReceivingMailbox>>,
    /// Current index for round-robin
    current_index: AtomicUsize,
    /// Number of active mailboxes
    active_count: AtomicUsize,
    /// Statistics
    stats: BlockStats,
}

impl BlockingMultiStreamConsumer {
    /// Create a new multi-stream consumer
    pub fn new(mailboxes: Vec<Arc<ReceivingMailbox>>) -> Self {
        let active_count = mailboxes.len();
        Self {
            mailboxes,
            current_index: AtomicUsize::new(0),
            active_count: AtomicUsize::new(active_count),
            stats: BlockStats::default(),
        }
    }

    /// Read next block from any mailbox (blocking)
    pub fn read_blocking(&self, timeout: Duration) -> Result<Option<MseBlock>> {
        let start = Instant::now();
        let deadline = start + timeout;
        let count = self.mailboxes.len();

        if count == 0 || self.active_count.load(Ordering::Relaxed) == 0 {
            return Ok(None);
        }

        // Fair round-robin across mailboxes
        loop {
            for _ in 0..count {
                let idx = self.current_index.fetch_add(1, Ordering::Relaxed) % count;
                let mailbox = &self.mailboxes[idx];

                if mailbox.state() == MailboxState::Closed {
                    continue;
                }

                if let Some(block) = mailbox.poll() {
                    if block.is_eos() {
                        self.active_count.fetch_sub(1, Ordering::Relaxed);
                    }
                    return Ok(Some(block));
                }
            }

            // Check timeout
            if Instant::now() >= deadline {
                return Ok(None);
            }

            // Brief sleep before retry
            std::thread::sleep(Duration::from_micros(100));
        }
    }

    /// Async read from any mailbox
    pub async fn read_async(&self, timeout: Duration) -> Result<Option<MseBlock>> {
        let count = self.mailboxes.len();
        if count == 0 || self.active_count.load(Ordering::Relaxed) == 0 {
            return Ok(None);
        }

        // Try each mailbox once
        for _ in 0..count {
            let idx = self.current_index.fetch_add(1, Ordering::Relaxed) % count;
            let mailbox = &self.mailboxes[idx];

            if mailbox.state() == MailboxState::Closed {
                continue;
            }

            if let Some(block) = mailbox.poll() {
                if block.is_eos() {
                    self.active_count.fetch_sub(1, Ordering::Relaxed);
                }
                return Ok(Some(block));
            }
        }

        // Wait on first mailbox with data
        let idx = self.current_index.load(Ordering::Relaxed) % count;
        self.mailboxes[idx].receive_async(timeout).await
    }

    /// Get number of active mailboxes
    pub fn active_count(&self) -> usize {
        self.active_count.load(Ordering::Relaxed)
    }

    /// Get accumulated statistics
    pub fn stats(&self) -> &BlockStats {
        &self.stats
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::block::{BlockId, BlockSchema, ColumnSchema, ColumnType, DataBlock, DataBlockBuilder};

    fn create_test_block(query_id: &str, stage_id: i32, seq: u64) -> MseBlock {
        let schema = Arc::new(BlockSchema::new(vec![ColumnSchema {
            name: "id".to_string(),
            data_type: ColumnType::Int,
            nullable: false,
        }]));
        let id = BlockId::new(query_id, stage_id, 0, seq);
        let block = DataBlockBuilder::new(id, schema)
            .add_int_column(vec![1, 2, 3])
            .build();
        MseBlock::Data(block)
    }

    #[test]
    fn test_mailbox_id() {
        let id = MailboxId::new("q1", 0, 0, 1, 0);
        assert_eq!(id.to_string(), "q1:0:0->1:0");
    }

    #[test]
    fn test_receiving_mailbox_basic() {
        let id = MailboxId::new("q1", 0, 0, 1, 0);
        let mailbox = ReceivingMailbox::new(id, 5);

        assert_eq!(mailbox.state(), MailboxState::Open);
        assert_eq!(mailbox.pending_blocks(), 0);
        assert!(!mailbox.is_full());
    }

    #[test]
    fn test_mailbox_offer_and_receive() {
        let id = MailboxId::new("q1", 0, 0, 1, 0);
        let mailbox = ReceivingMailbox::new(id, 5);

        let block = create_test_block("q1", 0, 1);
        mailbox.offer(block, Duration::from_secs(1)).unwrap();

        assert_eq!(mailbox.pending_blocks(), 1);

        let received = mailbox.receive(Duration::from_secs(1)).unwrap();
        assert!(received.is_some());
        assert_eq!(mailbox.pending_blocks(), 0);
    }

    #[test]
    fn test_mailbox_backpressure() {
        let id = MailboxId::new("q1", 0, 0, 1, 0);
        let mailbox = ReceivingMailbox::new(id, 2);

        // Fill the mailbox
        for i in 0..2 {
            let block = create_test_block("q1", 0, i);
            mailbox.offer(block, Duration::from_secs(1)).unwrap();
        }

        assert!(mailbox.is_full());

        // Next offer should timeout
        let block = create_test_block("q1", 0, 3);
        let result = mailbox.offer(block, Duration::from_millis(10));
        assert!(result.is_err());
    }

    #[test]
    fn test_mailbox_eos() {
        let id = MailboxId::new("q1", 0, 0, 1, 0);
        let mailbox = ReceivingMailbox::new(id.clone(), 5);

        let eos = MseBlock::Eos {
            query_id: "q1".to_string(),
            stage_id: 0,
            stats: None,
        };
        mailbox.offer(eos, Duration::from_secs(1)).unwrap();

        let received = mailbox.receive(Duration::from_secs(1)).unwrap();
        assert!(received.unwrap().is_eos());
        assert_eq!(mailbox.state(), MailboxState::Closed);
    }

    #[test]
    fn test_sending_mailbox_local() {
        let id = MailboxId::new("q1", 0, 0, 1, 0);
        let receiver = Arc::new(ReceivingMailbox::new(id.clone(), 5));
        let sender = SendingMailbox::new_local(id, receiver.clone());

        assert!(sender.is_local());
        assert!(!sender.is_closed());

        let block = create_test_block("q1", 0, 1);
        sender.send(block, Duration::from_secs(1)).unwrap();

        let received = receiver.receive(Duration::from_secs(1)).unwrap();
        assert!(received.is_some());
    }

    #[test]
    fn test_mailbox_service() {
        let config = Arc::new(QueryServerConfig::default());
        let service = MailboxService::new(config);

        let id = MailboxId::new("q1", 0, 0, 1, 0);
        let mailbox = service.get_or_create_receiving_mailbox(id.clone());

        assert!(service.get_receiving_mailbox(&id).is_some());

        let (rx_count, tx_count) = service.mailbox_count();
        assert_eq!(rx_count, 1);
        assert_eq!(tx_count, 0);

        service.remove_query_mailboxes("q1");
        assert!(service.get_receiving_mailbox(&id).is_none());
    }

    #[test]
    fn test_multi_stream_consumer() {
        let id1 = MailboxId::new("q1", 0, 0, 1, 0);
        let id2 = MailboxId::new("q1", 0, 1, 1, 0);

        let mailbox1 = Arc::new(ReceivingMailbox::new(id1, 5));
        let mailbox2 = Arc::new(ReceivingMailbox::new(id2, 5));

        // Add blocks to both mailboxes
        let block1 = create_test_block("q1", 0, 1);
        let block2 = create_test_block("q1", 0, 2);
        mailbox1.offer(block1, Duration::from_secs(1)).unwrap();
        mailbox2.offer(block2, Duration::from_secs(1)).unwrap();

        let consumer = BlockingMultiStreamConsumer::new(vec![mailbox1, mailbox2]);

        // Should get blocks from both mailboxes
        let b1 = consumer.read_blocking(Duration::from_secs(1)).unwrap();
        let b2 = consumer.read_blocking(Duration::from_secs(1)).unwrap();

        assert!(b1.is_some());
        assert!(b2.is_some());
    }

    #[test]
    fn test_early_termination() {
        let id = MailboxId::new("q1", 0, 0, 1, 0);
        let mailbox = ReceivingMailbox::new(id, 5);

        mailbox.signal_early_termination();
        assert!(mailbox.is_early_terminated());

        let block = create_test_block("q1", 0, 1);
        let result = mailbox.offer(block, Duration::from_secs(1));
        assert!(result.is_err());
    }
}
