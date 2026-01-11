//! Data types supported by dictionaries

use std::cmp::Ordering;
use std::hash::{Hash, Hasher};
use ordered_float::OrderedFloat;
use serde::{Deserialize, Serialize};

/// Supported data types for dictionary encoding
#[derive(Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum DataType {
    Int,
    Long,
    Float,
    Double,
    String,
    Bytes,
    BigDecimal,
}

impl DataType {
    /// Returns the fixed size in bytes for this type, or None for variable-length types
    pub fn fixed_size(&self) -> Option<usize> {
        match self {
            DataType::Int => Some(4),
            DataType::Long => Some(8),
            DataType::Float => Some(4),
            DataType::Double => Some(8),
            DataType::String => None,
            DataType::Bytes => None,
            DataType::BigDecimal => None,
        }
    }

    /// Returns true if this is a numeric type
    pub fn is_numeric(&self) -> bool {
        matches!(
            self,
            DataType::Int | DataType::Long | DataType::Float | DataType::Double | DataType::BigDecimal
        )
    }
}

/// A value that can be stored in a dictionary
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum DictionaryValue {
    Int(i32),
    Long(i64),
    Float(OrderedFloat<f32>),
    Double(OrderedFloat<f64>),
    String(String),
    Bytes(Vec<u8>),
}

impl DictionaryValue {
    /// Creates an Int value
    pub fn int(v: i32) -> Self {
        DictionaryValue::Int(v)
    }

    /// Creates a Long value
    pub fn long(v: i64) -> Self {
        DictionaryValue::Long(v)
    }

    /// Creates a Float value
    pub fn float(v: f32) -> Self {
        DictionaryValue::Float(OrderedFloat(v))
    }

    /// Creates a Double value
    pub fn double(v: f64) -> Self {
        DictionaryValue::Double(OrderedFloat(v))
    }

    /// Creates a String value
    pub fn string(v: impl Into<String>) -> Self {
        DictionaryValue::String(v.into())
    }

    /// Creates a Bytes value
    pub fn bytes(v: impl Into<Vec<u8>>) -> Self {
        DictionaryValue::Bytes(v.into())
    }

    /// Returns the data type of this value
    pub fn data_type(&self) -> DataType {
        match self {
            DictionaryValue::Int(_) => DataType::Int,
            DictionaryValue::Long(_) => DataType::Long,
            DictionaryValue::Float(_) => DataType::Float,
            DictionaryValue::Double(_) => DataType::Double,
            DictionaryValue::String(_) => DataType::String,
            DictionaryValue::Bytes(_) => DataType::Bytes,
        }
    }

    /// Converts to i32, with type coercion for numeric types
    pub fn as_int(&self) -> Option<i32> {
        match self {
            DictionaryValue::Int(v) => Some(*v),
            DictionaryValue::Long(v) => Some(*v as i32),
            DictionaryValue::Float(v) => Some(v.0 as i32),
            DictionaryValue::Double(v) => Some(v.0 as i32),
            _ => None,
        }
    }

    /// Converts to i64, with type coercion for numeric types
    pub fn as_long(&self) -> Option<i64> {
        match self {
            DictionaryValue::Int(v) => Some(*v as i64),
            DictionaryValue::Long(v) => Some(*v),
            DictionaryValue::Float(v) => Some(v.0 as i64),
            DictionaryValue::Double(v) => Some(v.0 as i64),
            _ => None,
        }
    }

    /// Converts to f32, with type coercion for numeric types
    pub fn as_float(&self) -> Option<f32> {
        match self {
            DictionaryValue::Int(v) => Some(*v as f32),
            DictionaryValue::Long(v) => Some(*v as f32),
            DictionaryValue::Float(v) => Some(v.0),
            DictionaryValue::Double(v) => Some(v.0 as f32),
            _ => None,
        }
    }

    /// Converts to f64, with type coercion for numeric types
    pub fn as_double(&self) -> Option<f64> {
        match self {
            DictionaryValue::Int(v) => Some(*v as f64),
            DictionaryValue::Long(v) => Some(*v as f64),
            DictionaryValue::Float(v) => Some(v.0 as f64),
            DictionaryValue::Double(v) => Some(v.0),
            _ => None,
        }
    }

    /// Returns the string representation
    pub fn as_string(&self) -> String {
        match self {
            DictionaryValue::Int(v) => v.to_string(),
            DictionaryValue::Long(v) => v.to_string(),
            DictionaryValue::Float(v) => v.0.to_string(),
            DictionaryValue::Double(v) => v.0.to_string(),
            DictionaryValue::String(v) => v.clone(),
            DictionaryValue::Bytes(v) => hex::encode(v),
        }
    }

    /// Returns bytes representation
    pub fn as_bytes(&self) -> Vec<u8> {
        match self {
            DictionaryValue::Int(v) => v.to_be_bytes().to_vec(),
            DictionaryValue::Long(v) => v.to_be_bytes().to_vec(),
            DictionaryValue::Float(v) => v.0.to_be_bytes().to_vec(),
            DictionaryValue::Double(v) => v.0.to_be_bytes().to_vec(),
            DictionaryValue::String(v) => v.as_bytes().to_vec(),
            DictionaryValue::Bytes(v) => v.clone(),
        }
    }

    /// Returns an ordering value for the type (used for cross-type comparisons)
    fn type_order(&self) -> u8 {
        match self {
            DictionaryValue::Int(_) => 0,
            DictionaryValue::Long(_) => 1,
            DictionaryValue::Float(_) => 2,
            DictionaryValue::Double(_) => 3,
            DictionaryValue::String(_) => 4,
            DictionaryValue::Bytes(_) => 5,
        }
    }
}

impl PartialEq for DictionaryValue {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (DictionaryValue::Int(a), DictionaryValue::Int(b)) => a == b,
            (DictionaryValue::Long(a), DictionaryValue::Long(b)) => a == b,
            (DictionaryValue::Float(a), DictionaryValue::Float(b)) => a == b,
            (DictionaryValue::Double(a), DictionaryValue::Double(b)) => a == b,
            (DictionaryValue::String(a), DictionaryValue::String(b)) => a == b,
            (DictionaryValue::Bytes(a), DictionaryValue::Bytes(b)) => a == b,
            _ => false,
        }
    }
}

impl Eq for DictionaryValue {}

impl Hash for DictionaryValue {
    fn hash<H: Hasher>(&self, state: &mut H) {
        std::mem::discriminant(self).hash(state);
        match self {
            DictionaryValue::Int(v) => v.hash(state),
            DictionaryValue::Long(v) => v.hash(state),
            DictionaryValue::Float(v) => v.hash(state),
            DictionaryValue::Double(v) => v.hash(state),
            DictionaryValue::String(v) => v.hash(state),
            DictionaryValue::Bytes(v) => v.hash(state),
        }
    }
}

impl PartialOrd for DictionaryValue {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for DictionaryValue {
    fn cmp(&self, other: &Self) -> Ordering {
        match (self, other) {
            (DictionaryValue::Int(a), DictionaryValue::Int(b)) => a.cmp(b),
            (DictionaryValue::Long(a), DictionaryValue::Long(b)) => a.cmp(b),
            (DictionaryValue::Float(a), DictionaryValue::Float(b)) => a.cmp(b),
            (DictionaryValue::Double(a), DictionaryValue::Double(b)) => a.cmp(b),
            (DictionaryValue::String(a), DictionaryValue::String(b)) => a.cmp(b),
            (DictionaryValue::Bytes(a), DictionaryValue::Bytes(b)) => a.cmp(b),
            // Cross-type comparison uses type order
            _ => self.type_order().cmp(&other.type_order()),
        }
    }
}

/// Helper module for hex encoding (simple implementation)
mod hex {
    const HEX_CHARS: &[u8; 16] = b"0123456789abcdef";

    pub fn encode(bytes: &[u8]) -> String {
        let mut result = String::with_capacity(bytes.len() * 2);
        for byte in bytes {
            result.push(HEX_CHARS[(byte >> 4) as usize] as char);
            result.push(HEX_CHARS[(byte & 0x0f) as usize] as char);
        }
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_data_type_fixed_size() {
        assert_eq!(DataType::Int.fixed_size(), Some(4));
        assert_eq!(DataType::Long.fixed_size(), Some(8));
        assert_eq!(DataType::Float.fixed_size(), Some(4));
        assert_eq!(DataType::Double.fixed_size(), Some(8));
        assert_eq!(DataType::String.fixed_size(), None);
        assert_eq!(DataType::Bytes.fixed_size(), None);
    }

    #[test]
    fn test_dictionary_value_type_coercion() {
        let int_val = DictionaryValue::int(42);
        assert_eq!(int_val.as_int(), Some(42));
        assert_eq!(int_val.as_long(), Some(42));
        assert_eq!(int_val.as_float(), Some(42.0));
        assert_eq!(int_val.as_double(), Some(42.0));

        let str_val = DictionaryValue::string("hello");
        assert_eq!(str_val.as_int(), None);
        assert_eq!(str_val.as_string(), "hello");
    }

    #[test]
    fn test_dictionary_value_equality() {
        let v1 = DictionaryValue::int(42);
        let v2 = DictionaryValue::int(42);
        let v3 = DictionaryValue::int(43);
        let v4 = DictionaryValue::long(42);

        assert_eq!(v1, v2);
        assert_ne!(v1, v3);
        assert_ne!(v1, v4); // Different types
    }

    #[test]
    fn test_dictionary_value_ordering() {
        let v1 = DictionaryValue::int(1);
        let v2 = DictionaryValue::int(2);
        let v3 = DictionaryValue::int(3);

        assert!(v1 < v2);
        assert!(v2 < v3);

        let s1 = DictionaryValue::string("apple");
        let s2 = DictionaryValue::string("banana");
        assert!(s1 < s2);
    }

    #[test]
    fn test_dictionary_value_hash() {
        use std::collections::HashSet;

        let mut set = HashSet::new();
        set.insert(DictionaryValue::int(1));
        set.insert(DictionaryValue::int(2));
        set.insert(DictionaryValue::int(1)); // Duplicate

        assert_eq!(set.len(), 2);
    }

    #[test]
    fn test_bytes_hex_encoding() {
        let bytes_val = DictionaryValue::bytes(vec![0xde, 0xad, 0xbe, 0xef]);
        assert_eq!(bytes_val.as_string(), "deadbeef");
    }

    #[test]
    fn test_float_ordering() {
        let f1 = DictionaryValue::float(1.0);
        let f2 = DictionaryValue::float(2.0);
        let f3 = DictionaryValue::float(1.0);

        assert!(f1 < f2);
        assert_eq!(f1, f3);
    }
}
