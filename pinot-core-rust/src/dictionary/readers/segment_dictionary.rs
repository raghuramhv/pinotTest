//! Segment-level dictionary implementation using ValueReader.
//!
//! Provides a dictionary abstraction over segment file data with:
//! - Binary search for sorted dictionaries
//! - Index-based value lookup (dict_id -> value)
//! - Value search (value -> dict_id)

use super::value_reader::{ValueReader, ValueReaderError};
use crate::dictionary::{DictionaryError, DictionaryResult, NULL_VALUE_INDEX};
use std::cmp::Ordering;
use std::sync::Arc;

/// Configuration for segment dictionary behavior.
#[derive(Debug, Clone)]
pub struct SegmentDictionaryConfig {
    /// Whether the dictionary values are sorted
    pub sorted: bool,
    /// Data type of the dictionary values
    pub data_type: SegmentDataType,
    /// Maximum number of entries to cache for reverse lookup
    pub max_cache_entries: usize,
}

impl Default for SegmentDictionaryConfig {
    fn default() -> Self {
        Self {
            sorted: true,
            data_type: SegmentDataType::Int,
            max_cache_entries: 10000,
        }
    }
}

/// Data types supported by segment dictionaries.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SegmentDataType {
    Int,
    Long,
    Float,
    Double,
    String,
    Bytes,
}

/// A dictionary backed by segment file data.
///
/// This is the main interface for reading dictionary values from Pinot segments.
/// It wraps a `ValueReader` and provides dictionary-specific operations.
pub struct SegmentDictionary {
    /// The underlying value reader
    reader: Arc<dyn ValueReader>,
    /// Dictionary configuration
    config: SegmentDictionaryConfig,
    /// Minimum value (for sorted dictionaries)
    min_value: Option<DictionaryValue>,
    /// Maximum value (for sorted dictionaries)
    max_value: Option<DictionaryValue>,
}

/// A dictionary value that can hold any supported type.
#[derive(Debug, Clone, PartialEq)]
pub enum DictionaryValue {
    Int(i32),
    Long(i64),
    Float(f32),
    Double(f64),
    String(String),
    Bytes(Vec<u8>),
}

impl DictionaryValue {
    pub fn as_int(&self) -> Option<i32> {
        match self {
            DictionaryValue::Int(v) => Some(*v),
            _ => None,
        }
    }

    pub fn as_long(&self) -> Option<i64> {
        match self {
            DictionaryValue::Long(v) => Some(*v),
            DictionaryValue::Int(v) => Some(*v as i64),
            _ => None,
        }
    }

    pub fn as_float(&self) -> Option<f32> {
        match self {
            DictionaryValue::Float(v) => Some(*v),
            DictionaryValue::Int(v) => Some(*v as f32),
            _ => None,
        }
    }

    pub fn as_double(&self) -> Option<f64> {
        match self {
            DictionaryValue::Double(v) => Some(*v),
            DictionaryValue::Float(v) => Some(*v as f64),
            DictionaryValue::Int(v) => Some(*v as f64),
            DictionaryValue::Long(v) => Some(*v as f64),
            _ => None,
        }
    }

    pub fn as_string(&self) -> Option<&str> {
        match self {
            DictionaryValue::String(v) => Some(v),
            _ => None,
        }
    }

    pub fn as_bytes(&self) -> Option<&[u8]> {
        match self {
            DictionaryValue::Bytes(v) => Some(v),
            DictionaryValue::String(v) => Some(v.as_bytes()),
            _ => None,
        }
    }
}

impl SegmentDictionary {
    /// Create a new segment dictionary.
    pub fn new(reader: Arc<dyn ValueReader>, config: SegmentDictionaryConfig) -> Self {
        let mut dict = Self {
            reader,
            config,
            min_value: None,
            max_value: None,
        };

        // Cache min/max for sorted dictionaries
        if dict.config.sorted && dict.len() > 0 {
            dict.min_value = dict.get_value(0).ok();
            dict.max_value = dict.get_value((dict.len() - 1) as i32).ok();
        }

        dict
    }

    /// Returns the number of entries in the dictionary.
    pub fn len(&self) -> usize {
        self.reader.len()
    }

    /// Returns true if the dictionary is empty.
    pub fn is_empty(&self) -> bool {
        self.reader.is_empty()
    }

    /// Returns the data type of the dictionary.
    pub fn data_type(&self) -> SegmentDataType {
        self.config.data_type
    }

    /// Get a value by dictionary ID.
    pub fn get_value(&self, dict_id: i32) -> DictionaryResult<DictionaryValue> {
        if dict_id < 0 || dict_id as usize >= self.len() {
            return Err(DictionaryError::InvalidDictId { dict_id });
        }

        let index = dict_id as usize;
        match self.config.data_type {
            SegmentDataType::Int => {
                let v = self.reader.get_int(index).map_err(reader_to_dict_error)?;
                Ok(DictionaryValue::Int(v))
            }
            SegmentDataType::Long => {
                let v = self.reader.get_long(index).map_err(reader_to_dict_error)?;
                Ok(DictionaryValue::Long(v))
            }
            SegmentDataType::Float => {
                let v = self.reader.get_float(index).map_err(reader_to_dict_error)?;
                Ok(DictionaryValue::Float(v))
            }
            SegmentDataType::Double => {
                let v = self.reader.get_double(index).map_err(reader_to_dict_error)?;
                Ok(DictionaryValue::Double(v))
            }
            SegmentDataType::String => {
                let v = self.reader.get_string(index).map_err(reader_to_dict_error)?;
                Ok(DictionaryValue::String(v))
            }
            SegmentDataType::Bytes => {
                let v = self.reader.get_bytes(index).map_err(reader_to_dict_error)?;
                Ok(DictionaryValue::Bytes(v))
            }
        }
    }

    /// Get an i32 value by dictionary ID.
    pub fn get_int(&self, dict_id: i32) -> DictionaryResult<i32> {
        if dict_id < 0 || dict_id as usize >= self.len() {
            return Err(DictionaryError::InvalidDictId { dict_id });
        }
        self.reader
            .get_int(dict_id as usize)
            .map_err(reader_to_dict_error)
    }

    /// Get an i64 value by dictionary ID.
    pub fn get_long(&self, dict_id: i32) -> DictionaryResult<i64> {
        if dict_id < 0 || dict_id as usize >= self.len() {
            return Err(DictionaryError::InvalidDictId { dict_id });
        }
        self.reader
            .get_long(dict_id as usize)
            .map_err(reader_to_dict_error)
    }

    /// Get an f64 value by dictionary ID.
    pub fn get_double(&self, dict_id: i32) -> DictionaryResult<f64> {
        if dict_id < 0 || dict_id as usize >= self.len() {
            return Err(DictionaryError::InvalidDictId { dict_id });
        }
        self.reader
            .get_double(dict_id as usize)
            .map_err(reader_to_dict_error)
    }

    /// Get a string value by dictionary ID.
    pub fn get_string(&self, dict_id: i32) -> DictionaryResult<String> {
        if dict_id < 0 || dict_id as usize >= self.len() {
            return Err(DictionaryError::InvalidDictId { dict_id });
        }
        self.reader
            .get_string(dict_id as usize)
            .map_err(reader_to_dict_error)
    }

    /// Get bytes by dictionary ID.
    pub fn get_bytes(&self, dict_id: i32) -> DictionaryResult<Vec<u8>> {
        if dict_id < 0 || dict_id as usize >= self.len() {
            return Err(DictionaryError::InvalidDictId { dict_id });
        }
        self.reader
            .get_bytes(dict_id as usize)
            .map_err(reader_to_dict_error)
    }

    /// Find the dictionary ID for an i32 value.
    /// Returns NULL_VALUE_INDEX if not found.
    pub fn index_of_int(&self, value: i32) -> i32 {
        if self.config.sorted {
            self.binary_search_int(value)
        } else {
            self.linear_search_int(value)
        }
    }

    /// Find the dictionary ID for an i64 value.
    pub fn index_of_long(&self, value: i64) -> i32 {
        if self.config.sorted {
            self.binary_search_long(value)
        } else {
            self.linear_search_long(value)
        }
    }

    /// Find the dictionary ID for a string value.
    pub fn index_of_string(&self, value: &str) -> i32 {
        if self.config.sorted {
            self.binary_search_string(value)
        } else {
            self.linear_search_string(value)
        }
    }

    /// Find the dictionary ID for a bytes value.
    pub fn index_of_bytes(&self, value: &[u8]) -> i32 {
        if self.config.sorted {
            self.binary_search_bytes(value)
        } else {
            self.linear_search_bytes(value)
        }
    }

    /// Get the insertion index for a value (for sorted dictionaries).
    /// Returns the index where the value should be inserted to maintain order.
    pub fn insertion_index_of_int(&self, value: i32) -> i32 {
        if !self.config.sorted {
            return NULL_VALUE_INDEX;
        }

        let len = self.len();
        if len == 0 {
            return 0;
        }

        let mut low = 0;
        let mut high = len;

        while low < high {
            let mid = low + (high - low) / 2;
            if let Ok(mid_val) = self.reader.get_int(mid) {
                match mid_val.cmp(&value) {
                    Ordering::Less => low = mid + 1,
                    Ordering::Greater => high = mid,
                    Ordering::Equal => return mid as i32,
                }
            } else {
                return NULL_VALUE_INDEX;
            }
        }

        low as i32
    }

    /// Get the minimum value (for sorted dictionaries).
    pub fn min_value(&self) -> Option<&DictionaryValue> {
        self.min_value.as_ref()
    }

    /// Get the maximum value (for sorted dictionaries).
    pub fn max_value(&self) -> Option<&DictionaryValue> {
        self.max_value.as_ref()
    }

    /// Read multiple int values in batch.
    pub fn read_int_values(&self, dict_ids: &[i32], output: &mut [i32]) -> DictionaryResult<()> {
        let indices: Vec<usize> = dict_ids.iter().map(|&id| id as usize).collect();
        self.reader
            .get_int_batch(&indices, output)
            .map_err(reader_to_dict_error)
    }

    /// Read multiple long values in batch.
    pub fn read_long_values(&self, dict_ids: &[i32], output: &mut [i64]) -> DictionaryResult<()> {
        let indices: Vec<usize> = dict_ids.iter().map(|&id| id as usize).collect();
        self.reader
            .get_long_batch(&indices, output)
            .map_err(reader_to_dict_error)
    }

    /// Read multiple double values in batch.
    pub fn read_double_values(&self, dict_ids: &[i32], output: &mut [f64]) -> DictionaryResult<()> {
        let indices: Vec<usize> = dict_ids.iter().map(|&id| id as usize).collect();
        self.reader
            .get_double_batch(&indices, output)
            .map_err(reader_to_dict_error)
    }

    // Binary search implementations for sorted dictionaries

    fn binary_search_int(&self, value: i32) -> i32 {
        let len = self.len();
        if len == 0 {
            return NULL_VALUE_INDEX;
        }

        let mut low = 0;
        let mut high = len - 1;

        while low <= high {
            let mid = low + (high - low) / 2;
            if let Ok(mid_val) = self.reader.get_int(mid) {
                match mid_val.cmp(&value) {
                    Ordering::Equal => return mid as i32,
                    Ordering::Less => low = mid + 1,
                    Ordering::Greater => {
                        if mid == 0 {
                            break;
                        }
                        high = mid - 1;
                    }
                }
            } else {
                return NULL_VALUE_INDEX;
            }
        }

        NULL_VALUE_INDEX
    }

    fn binary_search_long(&self, value: i64) -> i32 {
        let len = self.len();
        if len == 0 {
            return NULL_VALUE_INDEX;
        }

        let mut low = 0;
        let mut high = len - 1;

        while low <= high {
            let mid = low + (high - low) / 2;
            if let Ok(mid_val) = self.reader.get_long(mid) {
                match mid_val.cmp(&value) {
                    Ordering::Equal => return mid as i32,
                    Ordering::Less => low = mid + 1,
                    Ordering::Greater => {
                        if mid == 0 {
                            break;
                        }
                        high = mid - 1;
                    }
                }
            } else {
                return NULL_VALUE_INDEX;
            }
        }

        NULL_VALUE_INDEX
    }

    fn binary_search_string(&self, value: &str) -> i32 {
        let value_bytes = value.as_bytes();
        self.binary_search_bytes(value_bytes)
    }

    fn binary_search_bytes(&self, value: &[u8]) -> i32 {
        let len = self.len();
        if len == 0 {
            return NULL_VALUE_INDEX;
        }

        let mut low = 0;
        let mut high = len - 1;

        while low <= high {
            let mid = low + (high - low) / 2;
            if let Ok(cmp) = self.reader.compare_with_bytes(mid, value) {
                match cmp {
                    Ordering::Equal => return mid as i32,
                    Ordering::Less => low = mid + 1,
                    Ordering::Greater => {
                        if mid == 0 {
                            break;
                        }
                        high = mid - 1;
                    }
                }
            } else {
                return NULL_VALUE_INDEX;
            }
        }

        NULL_VALUE_INDEX
    }

    // Linear search implementations for unsorted dictionaries

    fn linear_search_int(&self, value: i32) -> i32 {
        for i in 0..self.len() {
            if let Ok(v) = self.reader.get_int(i) {
                if v == value {
                    return i as i32;
                }
            }
        }
        NULL_VALUE_INDEX
    }

    fn linear_search_long(&self, value: i64) -> i32 {
        for i in 0..self.len() {
            if let Ok(v) = self.reader.get_long(i) {
                if v == value {
                    return i as i32;
                }
            }
        }
        NULL_VALUE_INDEX
    }

    fn linear_search_string(&self, value: &str) -> i32 {
        for i in 0..self.len() {
            if let Ok(v) = self.reader.get_string(i) {
                if v == value {
                    return i as i32;
                }
            }
        }
        NULL_VALUE_INDEX
    }

    fn linear_search_bytes(&self, value: &[u8]) -> i32 {
        for i in 0..self.len() {
            if let Ok(v) = self.reader.get_bytes(i) {
                if v == value {
                    return i as i32;
                }
            }
        }
        NULL_VALUE_INDEX
    }
}

fn reader_to_dict_error(e: ValueReaderError) -> DictionaryError {
    match e {
        ValueReaderError::IndexOutOfBounds { index, length } => {
            DictionaryError::IndexOutOfBounds { index, length }
        }
        _ => DictionaryError::SerializationError(e.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::super::fixed_byte_reader::FixedByteValueReader;
    use super::super::var_length_reader::{VarLengthValueReader, VarLengthValueWriter};
    use super::*;

    fn create_sorted_int_dict() -> SegmentDictionary {
        // Create sorted int values: 10, 20, 30, 40, 50
        let mut data = Vec::new();
        for &val in &[10i32, 20, 30, 40, 50] {
            data.extend_from_slice(&val.to_be_bytes());
        }
        let reader = FixedByteValueReader::from_bytes(data, 5, 4);

        SegmentDictionary::new(
            Arc::new(reader),
            SegmentDictionaryConfig {
                sorted: true,
                data_type: SegmentDataType::Int,
                ..Default::default()
            },
        )
    }

    fn create_sorted_string_dict() -> SegmentDictionary {
        // Create sorted strings: apple, banana, cherry, date, elderberry
        let strings = ["apple", "banana", "cherry", "date", "elderberry"];
        let mut writer = VarLengthValueWriter::new();
        for s in &strings {
            writer.push_string(s);
        }
        let buffer = writer.build_offset_table();
        let reader = VarLengthValueReader::from_bytes_offset_table(buffer, strings.len()).unwrap();

        SegmentDictionary::new(
            Arc::new(reader),
            SegmentDictionaryConfig {
                sorted: true,
                data_type: SegmentDataType::String,
                ..Default::default()
            },
        )
    }

    #[test]
    fn test_get_int() {
        let dict = create_sorted_int_dict();
        assert_eq!(dict.get_int(0).unwrap(), 10);
        assert_eq!(dict.get_int(2).unwrap(), 30);
        assert_eq!(dict.get_int(4).unwrap(), 50);
    }

    #[test]
    fn test_get_int_invalid_id() {
        let dict = create_sorted_int_dict();
        assert!(dict.get_int(-1).is_err());
        assert!(dict.get_int(5).is_err());
        assert!(dict.get_int(100).is_err());
    }

    #[test]
    fn test_index_of_int() {
        let dict = create_sorted_int_dict();
        assert_eq!(dict.index_of_int(10), 0);
        assert_eq!(dict.index_of_int(30), 2);
        assert_eq!(dict.index_of_int(50), 4);
        assert_eq!(dict.index_of_int(25), NULL_VALUE_INDEX);
        assert_eq!(dict.index_of_int(100), NULL_VALUE_INDEX);
    }

    #[test]
    fn test_insertion_index() {
        let dict = create_sorted_int_dict();
        // Existing values
        assert_eq!(dict.insertion_index_of_int(10), 0);
        assert_eq!(dict.insertion_index_of_int(30), 2);
        // Non-existing values
        assert_eq!(dict.insertion_index_of_int(5), 0); // Before 10
        assert_eq!(dict.insertion_index_of_int(15), 1); // Between 10 and 20
        assert_eq!(dict.insertion_index_of_int(100), 5); // After 50
    }

    #[test]
    fn test_get_string() {
        let dict = create_sorted_string_dict();
        assert_eq!(dict.get_string(0).unwrap(), "apple");
        assert_eq!(dict.get_string(2).unwrap(), "cherry");
        assert_eq!(dict.get_string(4).unwrap(), "elderberry");
    }

    #[test]
    fn test_index_of_string() {
        let dict = create_sorted_string_dict();
        assert_eq!(dict.index_of_string("apple"), 0);
        assert_eq!(dict.index_of_string("cherry"), 2);
        assert_eq!(dict.index_of_string("elderberry"), 4);
        assert_eq!(dict.index_of_string("fig"), NULL_VALUE_INDEX);
    }

    #[test]
    fn test_get_value() {
        let dict = create_sorted_int_dict();
        let val = dict.get_value(2).unwrap();
        assert_eq!(val, DictionaryValue::Int(30));
        assert_eq!(val.as_int(), Some(30));
        assert_eq!(val.as_long(), Some(30));
    }

    #[test]
    fn test_min_max() {
        let dict = create_sorted_int_dict();
        assert_eq!(dict.min_value(), Some(&DictionaryValue::Int(10)));
        assert_eq!(dict.max_value(), Some(&DictionaryValue::Int(50)));
    }

    #[test]
    fn test_len() {
        let dict = create_sorted_int_dict();
        assert_eq!(dict.len(), 5);
        assert!(!dict.is_empty());
    }

    #[test]
    fn test_batch_read() {
        let dict = create_sorted_int_dict();
        let dict_ids = [0, 2, 4];
        let mut output = [0i32; 3];
        dict.read_int_values(&dict_ids, &mut output).unwrap();
        assert_eq!(output, [10, 30, 50]);
    }

    #[test]
    fn test_long_dict() {
        let mut data = Vec::new();
        for &val in &[100i64, 200, 300] {
            data.extend_from_slice(&val.to_be_bytes());
        }
        let reader = FixedByteValueReader::from_bytes(data, 3, 8);

        let dict = SegmentDictionary::new(
            Arc::new(reader),
            SegmentDictionaryConfig {
                sorted: true,
                data_type: SegmentDataType::Long,
                ..Default::default()
            },
        );

        assert_eq!(dict.get_long(0).unwrap(), 100);
        assert_eq!(dict.index_of_long(200), 1);
        assert_eq!(dict.index_of_long(999), NULL_VALUE_INDEX);
    }

    #[test]
    fn test_double_dict() {
        let mut data = Vec::new();
        for &val in &[1.5f64, 2.5, 3.5] {
            data.extend_from_slice(&val.to_be_bytes());
        }
        let reader = FixedByteValueReader::from_bytes(data, 3, 8);

        let dict = SegmentDictionary::new(
            Arc::new(reader),
            SegmentDictionaryConfig {
                sorted: true,
                data_type: SegmentDataType::Double,
                ..Default::default()
            },
        );

        assert_eq!(dict.get_double(0).unwrap(), 1.5);
        assert_eq!(dict.get_double(2).unwrap(), 3.5);
    }

    #[test]
    fn test_unsorted_dict() {
        // Unsorted values: 50, 10, 30, 20, 40
        let mut data = Vec::new();
        for &val in &[50i32, 10, 30, 20, 40] {
            data.extend_from_slice(&val.to_be_bytes());
        }
        let reader = FixedByteValueReader::from_bytes(data, 5, 4);

        let dict = SegmentDictionary::new(
            Arc::new(reader),
            SegmentDictionaryConfig {
                sorted: false,
                data_type: SegmentDataType::Int,
                ..Default::default()
            },
        );

        assert_eq!(dict.index_of_int(50), 0); // First element
        assert_eq!(dict.index_of_int(10), 1);
        assert_eq!(dict.index_of_int(40), 4); // Last element
        assert_eq!(dict.index_of_int(100), NULL_VALUE_INDEX);
    }

    #[test]
    fn test_empty_dict() {
        let reader = FixedByteValueReader::from_bytes(Vec::new(), 0, 4);
        let dict = SegmentDictionary::new(
            Arc::new(reader),
            SegmentDictionaryConfig {
                sorted: true,
                data_type: SegmentDataType::Int,
                ..Default::default()
            },
        );

        assert!(dict.is_empty());
        assert_eq!(dict.len(), 0);
        assert_eq!(dict.index_of_int(10), NULL_VALUE_INDEX);
        assert!(dict.min_value().is_none());
        assert!(dict.max_value().is_none());
    }
}
