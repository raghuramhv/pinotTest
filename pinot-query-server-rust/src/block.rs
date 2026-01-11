//! Data block types for query execution
//!
//! Blocks are the fundamental unit of data transfer in the scatter-gather model.
//! They represent chunks of query results that flow between operators and across
//! network boundaries.

use bytes::{Bytes, BytesMut};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::time::Instant;

use crate::error::{QueryError, Result};
use crate::MAX_BLOCK_SIZE_BYTES;

/// Unique identifier for a data block
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct BlockId {
    /// Query ID
    pub query_id: String,
    /// Stage ID
    pub stage_id: i32,
    /// Worker ID
    pub worker_id: i32,
    /// Sequence number within the stage
    pub sequence: u64,
}

impl BlockId {
    pub fn new(query_id: impl Into<String>, stage_id: i32, worker_id: i32, sequence: u64) -> Self {
        Self {
            query_id: query_id.into(),
            stage_id,
            worker_id,
            sequence,
        }
    }
}

impl std::fmt::Display for BlockId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}:{}:{}:{}",
            self.query_id, self.stage_id, self.worker_id, self.sequence
        )
    }
}

/// Column data types
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ColumnType {
    Int,
    Long,
    Float,
    Double,
    String,
    Bytes,
    Boolean,
    Timestamp,
    Json,
}

/// Column metadata
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ColumnSchema {
    pub name: String,
    pub data_type: ColumnType,
    pub nullable: bool,
}

/// Block schema describing the columns
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlockSchema {
    pub columns: Vec<ColumnSchema>,
}

impl BlockSchema {
    pub fn new(columns: Vec<ColumnSchema>) -> Self {
        Self { columns }
    }

    pub fn num_columns(&self) -> usize {
        self.columns.len()
    }
}

/// Column data storage
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ColumnData {
    Int(Vec<i32>),
    Long(Vec<i64>),
    Float(Vec<f32>),
    Double(Vec<f64>),
    String(Vec<String>),
    Bytes(Vec<Vec<u8>>),
    Boolean(Vec<bool>),
    /// Null bitmap for nullable columns (true = null)
    Nulls(Vec<bool>),
}

impl ColumnData {
    pub fn len(&self) -> usize {
        match self {
            ColumnData::Int(v) => v.len(),
            ColumnData::Long(v) => v.len(),
            ColumnData::Float(v) => v.len(),
            ColumnData::Double(v) => v.len(),
            ColumnData::String(v) => v.len(),
            ColumnData::Bytes(v) => v.len(),
            ColumnData::Boolean(v) => v.len(),
            ColumnData::Nulls(v) => v.len(),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Estimate memory size in bytes
    pub fn memory_size(&self) -> usize {
        match self {
            ColumnData::Int(v) => v.len() * 4,
            ColumnData::Long(v) => v.len() * 8,
            ColumnData::Float(v) => v.len() * 4,
            ColumnData::Double(v) => v.len() * 8,
            ColumnData::String(v) => v.iter().map(|s| s.len() + 24).sum(), // String overhead
            ColumnData::Bytes(v) => v.iter().map(|b| b.len() + 24).sum(),
            ColumnData::Boolean(v) => v.len(),
            ColumnData::Nulls(v) => v.len(),
        }
    }
}

/// A data block containing rows of columnar data
#[derive(Debug, Clone)]
pub struct DataBlock {
    /// Block identifier
    pub id: BlockId,
    /// Schema of the block
    pub schema: Arc<BlockSchema>,
    /// Column data
    pub columns: Vec<ColumnData>,
    /// Number of rows
    pub num_rows: usize,
    /// Null bitmaps per column (if any nulls present)
    pub null_bitmaps: Option<Vec<Vec<bool>>>,
    /// Block creation timestamp
    pub created_at: Option<Instant>,
}

/// Serializable form of DataBlock (schema not wrapped in Arc)
#[derive(Debug, Clone, Serialize, Deserialize)]
struct DataBlockSerde {
    id: BlockId,
    schema: BlockSchema,
    columns: Vec<ColumnData>,
    num_rows: usize,
    null_bitmaps: Option<Vec<Vec<bool>>>,
}

impl serde::Serialize for DataBlock {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let serde_form = DataBlockSerde {
            id: self.id.clone(),
            schema: (*self.schema).clone(),
            columns: self.columns.clone(),
            num_rows: self.num_rows,
            null_bitmaps: self.null_bitmaps.clone(),
        };
        serde_form.serialize(serializer)
    }
}

impl<'de> serde::Deserialize<'de> for DataBlock {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let serde_form = DataBlockSerde::deserialize(deserializer)?;
        Ok(DataBlock {
            id: serde_form.id,
            schema: Arc::new(serde_form.schema),
            columns: serde_form.columns,
            num_rows: serde_form.num_rows,
            null_bitmaps: serde_form.null_bitmaps,
            created_at: None,
        })
    }
}

impl DataBlock {
    /// Create a new data block
    pub fn new(id: BlockId, schema: Arc<BlockSchema>, columns: Vec<ColumnData>) -> Self {
        let num_rows = columns.first().map(|c| c.len()).unwrap_or(0);
        Self {
            id,
            schema,
            columns,
            num_rows,
            null_bitmaps: None,
            created_at: Some(Instant::now()),
        }
    }

    /// Create an empty data block with schema
    pub fn empty(id: BlockId, schema: Arc<BlockSchema>) -> Self {
        Self {
            id,
            schema,
            columns: Vec::new(),
            num_rows: 0,
            null_bitmaps: None,
            created_at: Some(Instant::now()),
        }
    }

    /// Get number of columns
    pub fn num_columns(&self) -> usize {
        self.columns.len()
    }

    /// Estimate memory size in bytes
    pub fn memory_size(&self) -> usize {
        let data_size: usize = self.columns.iter().map(|c| c.memory_size()).sum();
        let null_size: usize = self
            .null_bitmaps
            .as_ref()
            .map(|bitmaps| bitmaps.iter().map(|b| b.len()).sum())
            .unwrap_or(0);

        data_size + null_size + std::mem::size_of::<Self>()
    }

    /// Check if block is empty
    pub fn is_empty(&self) -> bool {
        self.num_rows == 0
    }

    /// Serialize to bytes
    pub fn serialize(&self) -> Result<Bytes> {
        let data = bincode::serialize(self)?;
        Ok(Bytes::from(data))
    }

    /// Deserialize from bytes
    pub fn deserialize(data: &[u8]) -> Result<Self> {
        let block: Self = bincode::deserialize(data)?;
        Ok(block)
    }

    /// Split block if larger than max size
    pub fn split_if_needed(self, max_size: usize) -> Vec<DataBlock> {
        if self.memory_size() <= max_size || self.num_rows <= 1 {
            return vec![self];
        }

        let rows_per_block = std::cmp::max(1, self.num_rows * max_size / self.memory_size());
        let num_blocks = (self.num_rows + rows_per_block - 1) / rows_per_block;

        let mut blocks = Vec::with_capacity(num_blocks);

        for i in 0..num_blocks {
            let start = i * rows_per_block;
            let end = std::cmp::min(start + rows_per_block, self.num_rows);

            let new_columns: Vec<ColumnData> = self
                .columns
                .iter()
                .map(|col| slice_column(col, start, end))
                .collect();

            let new_id = BlockId {
                query_id: self.id.query_id.clone(),
                stage_id: self.id.stage_id,
                worker_id: self.id.worker_id,
                sequence: self.id.sequence * 1000 + i as u64,
            };

            blocks.push(DataBlock::new(new_id, self.schema.clone(), new_columns));
        }

        blocks
    }
}

/// Slice a column from start to end index
fn slice_column(col: &ColumnData, start: usize, end: usize) -> ColumnData {
    match col {
        ColumnData::Int(v) => ColumnData::Int(v[start..end].to_vec()),
        ColumnData::Long(v) => ColumnData::Long(v[start..end].to_vec()),
        ColumnData::Float(v) => ColumnData::Float(v[start..end].to_vec()),
        ColumnData::Double(v) => ColumnData::Double(v[start..end].to_vec()),
        ColumnData::String(v) => ColumnData::String(v[start..end].to_vec()),
        ColumnData::Bytes(v) => ColumnData::Bytes(v[start..end].to_vec()),
        ColumnData::Boolean(v) => ColumnData::Boolean(v[start..end].to_vec()),
        ColumnData::Nulls(v) => ColumnData::Nulls(v[start..end].to_vec()),
    }
}

/// Message block types (mirrors Java MseBlock hierarchy)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum MseBlock {
    /// Data block with actual query results
    Data(DataBlock),
    /// End of stream marker
    Eos {
        query_id: String,
        stage_id: i32,
        /// Statistics from upstream
        stats: Option<BlockStats>,
    },
    /// Error block
    Error {
        query_id: String,
        stage_id: i32,
        error_code: i32,
        error_message: String,
    },
    /// Success marker (empty result)
    Success { query_id: String, stage_id: i32 },
}

impl MseBlock {
    /// Check if this is an end-of-stream block
    pub fn is_eos(&self) -> bool {
        matches!(self, MseBlock::Eos { .. })
    }

    /// Check if this is a data block
    pub fn is_data(&self) -> bool {
        matches!(self, MseBlock::Data(_))
    }

    /// Check if this is an error block
    pub fn is_error(&self) -> bool {
        matches!(self, MseBlock::Error { .. })
    }

    /// Get query ID
    pub fn query_id(&self) -> &str {
        match self {
            MseBlock::Data(block) => &block.id.query_id,
            MseBlock::Eos { query_id, .. } => query_id,
            MseBlock::Error { query_id, .. } => query_id,
            MseBlock::Success { query_id, .. } => query_id,
        }
    }

    /// Get stage ID
    pub fn stage_id(&self) -> i32 {
        match self {
            MseBlock::Data(block) => block.id.stage_id,
            MseBlock::Eos { stage_id, .. } => *stage_id,
            MseBlock::Error { stage_id, .. } => *stage_id,
            MseBlock::Success { stage_id, .. } => *stage_id,
        }
    }

    /// Estimate memory size
    pub fn memory_size(&self) -> usize {
        match self {
            MseBlock::Data(block) => block.memory_size(),
            MseBlock::Eos { stats, .. } => {
                std::mem::size_of::<Self>()
                    + stats.as_ref().map(|s| s.memory_size()).unwrap_or(0)
            }
            MseBlock::Error { error_message, .. } => {
                std::mem::size_of::<Self>() + error_message.len()
            }
            MseBlock::Success { .. } => std::mem::size_of::<Self>(),
        }
    }
}

/// Statistics collected during block processing
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct BlockStats {
    /// Execution time in milliseconds
    pub execution_time_ms: u64,
    /// Number of rows emitted
    pub emitted_rows: u64,
    /// Serialized bytes (for network transfer)
    pub serialized_bytes: u64,
    /// Serialization time in milliseconds
    pub serialization_time_ms: u64,
    /// Deserialized bytes
    pub deserialized_bytes: u64,
    /// Deserialization time in milliseconds
    pub deserialization_time_ms: u64,
    /// Downstream wait time in milliseconds (backpressure)
    pub downstream_wait_ms: u64,
    /// Upstream wait time in milliseconds
    pub upstream_wait_ms: u64,
    /// Number of in-memory messages (local transfer)
    pub in_memory_messages: u64,
    /// CPU time spent offering to queue
    pub offer_cpu_time_ms: u64,
    /// CPU time spent waiting on queue
    pub wait_cpu_time_ms: u64,
}

impl BlockStats {
    pub fn new() -> Self {
        Self::default()
    }

    /// Merge stats from another BlockStats
    pub fn merge(&mut self, other: &BlockStats) {
        self.execution_time_ms += other.execution_time_ms;
        self.emitted_rows += other.emitted_rows;
        self.serialized_bytes += other.serialized_bytes;
        self.serialization_time_ms += other.serialization_time_ms;
        self.deserialized_bytes += other.deserialized_bytes;
        self.deserialization_time_ms += other.deserialization_time_ms;
        self.downstream_wait_ms += other.downstream_wait_ms;
        self.upstream_wait_ms += other.upstream_wait_ms;
        self.in_memory_messages += other.in_memory_messages;
        self.offer_cpu_time_ms += other.offer_cpu_time_ms;
        self.wait_cpu_time_ms += other.wait_cpu_time_ms;
    }

    pub fn memory_size(&self) -> usize {
        std::mem::size_of::<Self>()
    }
}

/// Serialized block for network transmission
#[derive(Debug, Clone)]
pub struct SerializedBlock {
    /// Block header
    pub header: BlockHeader,
    /// Serialized data chunks
    pub chunks: Vec<Bytes>,
    /// Total serialized size
    pub total_size: usize,
}

/// Block header for network protocol
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlockHeader {
    pub query_id: String,
    pub stage_id: i32,
    pub worker_id: i32,
    pub sequence: u64,
    pub block_type: BlockType,
    pub num_chunks: usize,
    pub total_size: usize,
    pub checksum: u32,
}

/// Block type marker
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum BlockType {
    Data,
    Eos,
    Error,
    Success,
}

impl SerializedBlock {
    /// Serialize an MseBlock for network transmission
    pub fn from_mse_block(block: &MseBlock, max_chunk_size: usize) -> Result<Self> {
        let block_type = match block {
            MseBlock::Data(_) => BlockType::Data,
            MseBlock::Eos { .. } => BlockType::Eos,
            MseBlock::Error { .. } => BlockType::Error,
            MseBlock::Success { .. } => BlockType::Success,
        };

        let data = bincode::serialize(block)?;
        let total_size = data.len();
        let checksum = crc32fast::hash(&data);

        // Split into chunks
        let mut chunks = Vec::new();
        let mut offset = 0;
        while offset < total_size {
            let end = std::cmp::min(offset + max_chunk_size, total_size);
            chunks.push(Bytes::copy_from_slice(&data[offset..end]));
            offset = end;
        }

        let header = BlockHeader {
            query_id: block.query_id().to_string(),
            stage_id: block.stage_id(),
            worker_id: 0,
            sequence: 0,
            block_type,
            num_chunks: chunks.len(),
            total_size,
            checksum,
        };

        Ok(Self {
            header,
            chunks,
            total_size,
        })
    }

    /// Deserialize back to MseBlock
    pub fn to_mse_block(&self) -> Result<MseBlock> {
        // Reassemble chunks
        let mut data = BytesMut::with_capacity(self.total_size);
        for chunk in &self.chunks {
            data.extend_from_slice(chunk);
        }

        // Verify checksum
        let computed = crc32fast::hash(&data);
        if computed != self.header.checksum {
            return Err(QueryError::Deserialization(format!(
                "Checksum mismatch: expected {}, got {}",
                self.header.checksum, computed
            )));
        }

        let block: MseBlock = bincode::deserialize(&data)?;
        Ok(block)
    }
}

/// Builder for creating data blocks
pub struct DataBlockBuilder {
    id: BlockId,
    schema: Arc<BlockSchema>,
    columns: Vec<ColumnData>,
    null_bitmaps: Option<Vec<Vec<bool>>>,
}

impl DataBlockBuilder {
    pub fn new(id: BlockId, schema: Arc<BlockSchema>) -> Self {
        Self {
            id,
            schema,
            columns: Vec::new(),
            null_bitmaps: None,
        }
    }

    pub fn add_int_column(mut self, data: Vec<i32>) -> Self {
        self.columns.push(ColumnData::Int(data));
        self
    }

    pub fn add_long_column(mut self, data: Vec<i64>) -> Self {
        self.columns.push(ColumnData::Long(data));
        self
    }

    pub fn add_double_column(mut self, data: Vec<f64>) -> Self {
        self.columns.push(ColumnData::Double(data));
        self
    }

    pub fn add_string_column(mut self, data: Vec<String>) -> Self {
        self.columns.push(ColumnData::String(data));
        self
    }

    pub fn with_null_bitmaps(mut self, bitmaps: Vec<Vec<bool>>) -> Self {
        self.null_bitmaps = Some(bitmaps);
        self
    }

    pub fn build(self) -> DataBlock {
        let num_rows = self.columns.first().map(|c| c.len()).unwrap_or(0);
        DataBlock {
            id: self.id,
            schema: self.schema,
            columns: self.columns,
            num_rows,
            null_bitmaps: self.null_bitmaps,
            created_at: Some(Instant::now()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
                nullable: true,
            },
        ]))
    }

    #[test]
    fn test_block_creation() {
        let schema = create_test_schema();
        let id = BlockId::new("q1", 0, 0, 1);

        let block = DataBlockBuilder::new(id.clone(), schema)
            .add_int_column(vec![1, 2, 3])
            .add_string_column(vec!["a".to_string(), "b".to_string(), "c".to_string()])
            .build();

        assert_eq!(block.num_rows, 3);
        assert_eq!(block.num_columns(), 2);
        assert!(!block.is_empty());
    }

    #[test]
    fn test_block_serialization() {
        let schema = create_test_schema();
        let id = BlockId::new("q1", 0, 0, 1);

        let block = DataBlockBuilder::new(id, schema)
            .add_int_column(vec![1, 2, 3])
            .add_string_column(vec!["hello".to_string(), "world".to_string(), "!".to_string()])
            .build();

        let serialized = block.serialize().unwrap();
        let deserialized = DataBlock::deserialize(&serialized).unwrap();

        assert_eq!(deserialized.num_rows, 3);
        assert_eq!(deserialized.num_columns(), 2);
    }

    #[test]
    fn test_mse_block_types() {
        let eos = MseBlock::Eos {
            query_id: "q1".to_string(),
            stage_id: 0,
            stats: None,
        };
        assert!(eos.is_eos());
        assert!(!eos.is_data());

        let error = MseBlock::Error {
            query_id: "q1".to_string(),
            stage_id: 0,
            error_code: 500,
            error_message: "test error".to_string(),
        };
        assert!(error.is_error());
    }

    #[test]
    fn test_block_splitting() {
        let schema = create_test_schema();
        let id = BlockId::new("q1", 0, 0, 1);

        // Create a block with 100 rows
        let int_data: Vec<i32> = (0..100).collect();
        let string_data: Vec<String> = (0..100).map(|i| format!("value_{}", i)).collect();

        let block = DataBlockBuilder::new(id, schema)
            .add_int_column(int_data)
            .add_string_column(string_data)
            .build();

        // Split with small max size
        let blocks = block.split_if_needed(500);
        assert!(blocks.len() > 1);

        // Verify total rows preserved
        let total_rows: usize = blocks.iter().map(|b| b.num_rows).sum();
        assert_eq!(total_rows, 100);
    }

    #[test]
    fn test_serialized_block() {
        let schema = create_test_schema();
        let id = BlockId::new("q1", 0, 0, 1);

        let data_block = DataBlockBuilder::new(id, schema)
            .add_int_column(vec![1, 2, 3])
            .add_string_column(vec!["a".to_string(), "b".to_string(), "c".to_string()])
            .build();

        let mse_block = MseBlock::Data(data_block);
        let serialized = SerializedBlock::from_mse_block(&mse_block, 1024).unwrap();

        assert!(!serialized.chunks.is_empty());
        assert_eq!(serialized.header.block_type, BlockType::Data);

        let deserialized = serialized.to_mse_block().unwrap();
        assert!(deserialized.is_data());
    }

    #[test]
    fn test_block_stats_merge() {
        let mut stats1 = BlockStats {
            execution_time_ms: 100,
            emitted_rows: 1000,
            ..Default::default()
        };

        let stats2 = BlockStats {
            execution_time_ms: 50,
            emitted_rows: 500,
            ..Default::default()
        };

        stats1.merge(&stats2);

        assert_eq!(stats1.execution_time_ms, 150);
        assert_eq!(stats1.emitted_rows, 1500);
    }
}
