//! Dictionary traits defining the interface for dictionary operations

use super::{DataType, DictionaryError, DictionaryResult, DictionaryValue};
use std::collections::HashSet;

/// Sentinel value indicating a value was not found
pub const NULL_VALUE_INDEX: i32 = -1;

/// Core dictionary trait for immutable dictionaries
pub trait Dictionary: Send + Sync {
    /// Returns whether the dictionary values are sorted
    fn is_sorted(&self) -> bool;

    /// Returns the data type of values in this dictionary
    fn value_type(&self) -> DataType;

    /// Returns the number of entries in the dictionary
    fn len(&self) -> usize;

    /// Returns true if the dictionary is empty
    fn is_empty(&self) -> bool {
        self.len() == 0
    }

    // ========== Encoding: Value -> Dict ID ==========

    /// Returns the dictionary ID for the given value, or NULL_VALUE_INDEX if not found
    fn index_of(&self, value: &DictionaryValue) -> i32;

    /// Returns the dictionary ID for an int value
    fn index_of_int(&self, value: i32) -> i32 {
        self.index_of(&DictionaryValue::Int(value))
    }

    /// Returns the dictionary ID for a long value
    fn index_of_long(&self, value: i64) -> i32 {
        self.index_of(&DictionaryValue::Long(value))
    }

    /// Returns the dictionary ID for a float value
    fn index_of_float(&self, value: f32) -> i32 {
        self.index_of(&DictionaryValue::float(value))
    }

    /// Returns the dictionary ID for a double value
    fn index_of_double(&self, value: f64) -> i32 {
        self.index_of(&DictionaryValue::double(value))
    }

    /// Returns the dictionary ID for a string value
    fn index_of_string(&self, value: &str) -> i32 {
        self.index_of(&DictionaryValue::String(value.to_string()))
    }

    /// Returns the dictionary ID for a bytes value
    fn index_of_bytes(&self, value: &[u8]) -> i32 {
        self.index_of(&DictionaryValue::Bytes(value.to_vec()))
    }

    // ========== Decoding: Dict ID -> Value ==========

    /// Returns the value for the given dictionary ID
    fn get(&self, dict_id: i32) -> DictionaryResult<DictionaryValue>;

    /// Returns the int value for the given dictionary ID
    fn get_int(&self, dict_id: i32) -> DictionaryResult<i32> {
        self.get(dict_id)?
            .as_int()
            .ok_or_else(|| DictionaryError::TypeMismatch {
                expected: "Int".to_string(),
                actual: self.value_type().to_string(),
            })
    }

    /// Returns the long value for the given dictionary ID
    fn get_long(&self, dict_id: i32) -> DictionaryResult<i64> {
        self.get(dict_id)?
            .as_long()
            .ok_or_else(|| DictionaryError::TypeMismatch {
                expected: "Long".to_string(),
                actual: self.value_type().to_string(),
            })
    }

    /// Returns the float value for the given dictionary ID
    fn get_float(&self, dict_id: i32) -> DictionaryResult<f32> {
        self.get(dict_id)?
            .as_float()
            .ok_or_else(|| DictionaryError::TypeMismatch {
                expected: "Float".to_string(),
                actual: self.value_type().to_string(),
            })
    }

    /// Returns the double value for the given dictionary ID
    fn get_double(&self, dict_id: i32) -> DictionaryResult<f64> {
        self.get(dict_id)?
            .as_double()
            .ok_or_else(|| DictionaryError::TypeMismatch {
                expected: "Double".to_string(),
                actual: self.value_type().to_string(),
            })
    }

    /// Returns the string value for the given dictionary ID
    fn get_string(&self, dict_id: i32) -> DictionaryResult<String> {
        Ok(self.get(dict_id)?.as_string())
    }

    /// Returns the bytes value for the given dictionary ID
    fn get_bytes(&self, dict_id: i32) -> DictionaryResult<Vec<u8>> {
        Ok(self.get(dict_id)?.as_bytes())
    }

    // ========== Batch Operations ==========

    /// Reads int values for multiple dictionary IDs into the output buffer
    fn read_int_values(&self, dict_ids: &[i32], out: &mut [i32]) -> DictionaryResult<()> {
        for (i, &dict_id) in dict_ids.iter().enumerate() {
            out[i] = self.get_int(dict_id)?;
        }
        Ok(())
    }

    /// Reads long values for multiple dictionary IDs into the output buffer
    fn read_long_values(&self, dict_ids: &[i32], out: &mut [i64]) -> DictionaryResult<()> {
        for (i, &dict_id) in dict_ids.iter().enumerate() {
            out[i] = self.get_long(dict_id)?;
        }
        Ok(())
    }

    /// Reads float values for multiple dictionary IDs into the output buffer
    fn read_float_values(&self, dict_ids: &[i32], out: &mut [f32]) -> DictionaryResult<()> {
        for (i, &dict_id) in dict_ids.iter().enumerate() {
            out[i] = self.get_float(dict_id)?;
        }
        Ok(())
    }

    /// Reads double values for multiple dictionary IDs into the output buffer
    fn read_double_values(&self, dict_ids: &[i32], out: &mut [f64]) -> DictionaryResult<()> {
        for (i, &dict_id) in dict_ids.iter().enumerate() {
            out[i] = self.get_double(dict_id)?;
        }
        Ok(())
    }

    // ========== Range Operations ==========

    /// Returns dictionary IDs for values in the given range
    fn get_dict_ids_in_range(
        &self,
        lower: Option<&DictionaryValue>,
        upper: Option<&DictionaryValue>,
        include_lower: bool,
        include_upper: bool,
    ) -> HashSet<i32>;

    /// Compares two dictionary entries
    fn compare(&self, dict_id1: i32, dict_id2: i32) -> DictionaryResult<std::cmp::Ordering> {
        let v1 = self.get(dict_id1)?;
        let v2 = self.get(dict_id2)?;
        Ok(v1.cmp(&v2))
    }

    /// Returns the minimum value in the dictionary
    fn min_value(&self) -> Option<DictionaryValue>;

    /// Returns the maximum value in the dictionary
    fn max_value(&self) -> Option<DictionaryValue>;

    // ========== Binary Search (for sorted dictionaries) ==========

    /// Returns the insertion index for binary search
    /// Returns the index if found, or -(insertion_point + 1) if not found
    fn insertion_index_of(&self, value: &DictionaryValue) -> i32 {
        // Default implementation for unsorted dictionaries
        self.index_of(value)
    }
}

impl std::fmt::Debug for DataType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DataType::Int => write!(f, "INT"),
            DataType::Long => write!(f, "LONG"),
            DataType::Float => write!(f, "FLOAT"),
            DataType::Double => write!(f, "DOUBLE"),
            DataType::String => write!(f, "STRING"),
            DataType::Bytes => write!(f, "BYTES"),
            DataType::BigDecimal => write!(f, "BIG_DECIMAL"),
        }
    }
}

impl std::fmt::Display for DataType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        std::fmt::Debug::fmt(self, f)
    }
}

/// Trait for mutable dictionaries used in realtime segments
pub trait MutableDictionary: Dictionary {
    /// Indexes a value and returns its dictionary ID
    /// If the value already exists, returns the existing ID
    /// If the value is new, adds it and returns the new ID
    fn index(&mut self, value: DictionaryValue) -> DictionaryResult<i32>;

    /// Indexes multiple values and returns their dictionary IDs
    fn index_multiple(&mut self, values: &[DictionaryValue]) -> DictionaryResult<Vec<i32>> {
        values.iter().map(|v| self.index(v.clone())).collect()
    }

    /// Returns true if the dictionary can accept more entries
    fn can_add_more(&self) -> bool {
        true
    }
}

/// Trait for typed dictionary access (for better performance with known types)
pub trait TypedDictionary<T>: Dictionary
where
    T: Clone + Eq + std::hash::Hash,
{
    /// Returns the dictionary ID for the given typed value
    fn index_of_typed(&self, value: &T) -> i32;

    /// Returns the typed value for the given dictionary ID
    fn get_typed(&self, dict_id: i32) -> DictionaryResult<T>;

    /// Reads typed values for multiple dictionary IDs
    fn read_typed_values(&self, dict_ids: &[i32], out: &mut [T]) -> DictionaryResult<()>;
}

#[cfg(test)]
mod tests {
    use super::*;

    // Mock implementation for testing
    struct MockDictionary {
        values: Vec<DictionaryValue>,
    }

    impl Dictionary for MockDictionary {
        fn is_sorted(&self) -> bool {
            false
        }

        fn value_type(&self) -> DataType {
            DataType::Int
        }

        fn len(&self) -> usize {
            self.values.len()
        }

        fn index_of(&self, value: &DictionaryValue) -> i32 {
            self.values
                .iter()
                .position(|v| v == value)
                .map(|i| i as i32)
                .unwrap_or(NULL_VALUE_INDEX)
        }

        fn get(&self, dict_id: i32) -> DictionaryResult<DictionaryValue> {
            if dict_id < 0 || dict_id as usize >= self.values.len() {
                Err(DictionaryError::InvalidDictId { dict_id })
            } else {
                Ok(self.values[dict_id as usize].clone())
            }
        }

        fn get_dict_ids_in_range(
            &self,
            _lower: Option<&DictionaryValue>,
            _upper: Option<&DictionaryValue>,
            _include_lower: bool,
            _include_upper: bool,
        ) -> HashSet<i32> {
            HashSet::new()
        }

        fn min_value(&self) -> Option<DictionaryValue> {
            self.values.iter().min().cloned()
        }

        fn max_value(&self) -> Option<DictionaryValue> {
            self.values.iter().max().cloned()
        }
    }

    #[test]
    fn test_dictionary_trait() {
        let dict = MockDictionary {
            values: vec![
                DictionaryValue::int(10),
                DictionaryValue::int(20),
                DictionaryValue::int(30),
            ],
        };

        assert_eq!(dict.len(), 3);
        assert!(!dict.is_empty());
        assert!(!dict.is_sorted());

        // Test index_of
        assert_eq!(dict.index_of_int(10), 0);
        assert_eq!(dict.index_of_int(20), 1);
        assert_eq!(dict.index_of_int(30), 2);
        assert_eq!(dict.index_of_int(40), NULL_VALUE_INDEX);

        // Test get
        assert_eq!(dict.get_int(0).unwrap(), 10);
        assert_eq!(dict.get_int(1).unwrap(), 20);
        assert_eq!(dict.get_int(2).unwrap(), 30);
        assert!(dict.get_int(-1).is_err());
        assert!(dict.get_int(3).is_err());
    }

    #[test]
    fn test_batch_read() {
        let dict = MockDictionary {
            values: vec![
                DictionaryValue::int(100),
                DictionaryValue::int(200),
                DictionaryValue::int(300),
            ],
        };

        let dict_ids = [0, 1, 2];
        let mut out = [0i32; 3];
        dict.read_int_values(&dict_ids, &mut out).unwrap();

        assert_eq!(out, [100, 200, 300]);
    }
}
