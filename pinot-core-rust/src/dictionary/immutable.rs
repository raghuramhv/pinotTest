//! Immutable dictionary implementations for offline segments
//!
//! These dictionaries are pre-built and sorted, providing O(1) lookup
//! for both encoding (value -> dict_id) and decoding (dict_id -> value).

use super::{DataType, Dictionary, DictionaryError, DictionaryResult, DictionaryValue, NULL_VALUE_INDEX};
use ahash::AHashMap;
use ordered_float::OrderedFloat;
use std::collections::HashSet;

/// On-heap immutable dictionary for INT values
#[derive(Debug)]
pub struct OnHeapIntDictionary {
    /// Dict ID -> Value mapping (array for O(1) lookup)
    dict_id_to_value: Vec<i32>,
    /// Value -> Dict ID mapping (hash map for O(1) lookup)
    value_to_dict_id: AHashMap<i32, i32>,
    /// Minimum value
    min: Option<i32>,
    /// Maximum value
    max: Option<i32>,
}

impl OnHeapIntDictionary {
    /// Creates a new immutable int dictionary from sorted values
    pub fn new(values: Vec<i32>) -> Self {
        let len = values.len();
        let mut value_to_dict_id = AHashMap::with_capacity(len);

        for (dict_id, &value) in values.iter().enumerate() {
            value_to_dict_id.insert(value, dict_id as i32);
        }

        let min = values.first().copied();
        let max = values.last().copied();

        Self {
            dict_id_to_value: values,
            value_to_dict_id,
            min,
            max,
        }
    }

    /// Creates from unsorted values (will be sorted internally)
    pub fn from_unsorted(mut values: Vec<i32>) -> Self {
        values.sort_unstable();
        values.dedup();
        Self::new(values)
    }
}

impl Dictionary for OnHeapIntDictionary {
    fn is_sorted(&self) -> bool {
        true
    }

    fn value_type(&self) -> DataType {
        DataType::Int
    }

    fn len(&self) -> usize {
        self.dict_id_to_value.len()
    }

    fn index_of(&self, value: &DictionaryValue) -> i32 {
        match value {
            DictionaryValue::Int(v) => self.index_of_int(*v),
            _ => NULL_VALUE_INDEX,
        }
    }

    fn index_of_int(&self, value: i32) -> i32 {
        self.value_to_dict_id
            .get(&value)
            .copied()
            .unwrap_or(NULL_VALUE_INDEX)
    }

    fn get(&self, dict_id: i32) -> DictionaryResult<DictionaryValue> {
        self.get_int(dict_id).map(DictionaryValue::Int)
    }

    fn get_int(&self, dict_id: i32) -> DictionaryResult<i32> {
        if dict_id < 0 || dict_id as usize >= self.dict_id_to_value.len() {
            return Err(DictionaryError::InvalidDictId { dict_id });
        }
        Ok(self.dict_id_to_value[dict_id as usize])
    }

    fn read_int_values(&self, dict_ids: &[i32], out: &mut [i32]) -> DictionaryResult<()> {
        for (i, &dict_id) in dict_ids.iter().enumerate() {
            if dict_id < 0 || dict_id as usize >= self.dict_id_to_value.len() {
                return Err(DictionaryError::InvalidDictId { dict_id });
            }
            out[i] = self.dict_id_to_value[dict_id as usize];
        }
        Ok(())
    }

    fn get_dict_ids_in_range(
        &self,
        lower: Option<&DictionaryValue>,
        upper: Option<&DictionaryValue>,
        include_lower: bool,
        include_upper: bool,
    ) -> HashSet<i32> {
        let lower_val = lower.and_then(|v| v.as_int());
        let upper_val = upper.and_then(|v| v.as_int());

        let mut result = HashSet::new();

        // Binary search for start position
        let start_idx = match lower_val {
            Some(lower) => {
                let idx = self.dict_id_to_value.partition_point(|&x| x < lower);
                if include_lower {
                    idx
                } else {
                    self.dict_id_to_value.partition_point(|&x| x <= lower)
                }
            }
            None => 0,
        };

        // Binary search for end position
        let end_idx = match upper_val {
            Some(upper) => {
                if include_upper {
                    self.dict_id_to_value.partition_point(|&x| x <= upper)
                } else {
                    self.dict_id_to_value.partition_point(|&x| x < upper)
                }
            }
            None => self.dict_id_to_value.len(),
        };

        for dict_id in start_idx..end_idx {
            result.insert(dict_id as i32);
        }

        result
    }

    fn min_value(&self) -> Option<DictionaryValue> {
        self.min.map(DictionaryValue::Int)
    }

    fn max_value(&self) -> Option<DictionaryValue> {
        self.max.map(DictionaryValue::Int)
    }

    fn insertion_index_of(&self, value: &DictionaryValue) -> i32 {
        if let Some(int_val) = value.as_int() {
            match self.dict_id_to_value.binary_search(&int_val) {
                Ok(idx) => idx as i32,
                Err(idx) => -(idx as i32 + 1),
            }
        } else {
            NULL_VALUE_INDEX
        }
    }
}

/// On-heap immutable dictionary for LONG values
#[derive(Debug)]
pub struct OnHeapLongDictionary {
    dict_id_to_value: Vec<i64>,
    value_to_dict_id: AHashMap<i64, i32>,
    min: Option<i64>,
    max: Option<i64>,
}

impl OnHeapLongDictionary {
    pub fn new(values: Vec<i64>) -> Self {
        let len = values.len();
        let mut value_to_dict_id = AHashMap::with_capacity(len);

        for (dict_id, &value) in values.iter().enumerate() {
            value_to_dict_id.insert(value, dict_id as i32);
        }

        let min = values.first().copied();
        let max = values.last().copied();

        Self {
            dict_id_to_value: values,
            value_to_dict_id,
            min,
            max,
        }
    }

    pub fn from_unsorted(mut values: Vec<i64>) -> Self {
        values.sort_unstable();
        values.dedup();
        Self::new(values)
    }
}

impl Dictionary for OnHeapLongDictionary {
    fn is_sorted(&self) -> bool {
        true
    }

    fn value_type(&self) -> DataType {
        DataType::Long
    }

    fn len(&self) -> usize {
        self.dict_id_to_value.len()
    }

    fn index_of(&self, value: &DictionaryValue) -> i32 {
        match value {
            DictionaryValue::Long(v) => self.index_of_long(*v),
            DictionaryValue::Int(v) => self.index_of_long(*v as i64),
            _ => NULL_VALUE_INDEX,
        }
    }

    fn index_of_long(&self, value: i64) -> i32 {
        self.value_to_dict_id
            .get(&value)
            .copied()
            .unwrap_or(NULL_VALUE_INDEX)
    }

    fn get(&self, dict_id: i32) -> DictionaryResult<DictionaryValue> {
        self.get_long(dict_id).map(DictionaryValue::Long)
    }

    fn get_long(&self, dict_id: i32) -> DictionaryResult<i64> {
        if dict_id < 0 || dict_id as usize >= self.dict_id_to_value.len() {
            return Err(DictionaryError::InvalidDictId { dict_id });
        }
        Ok(self.dict_id_to_value[dict_id as usize])
    }

    fn read_long_values(&self, dict_ids: &[i32], out: &mut [i64]) -> DictionaryResult<()> {
        for (i, &dict_id) in dict_ids.iter().enumerate() {
            if dict_id < 0 || dict_id as usize >= self.dict_id_to_value.len() {
                return Err(DictionaryError::InvalidDictId { dict_id });
            }
            out[i] = self.dict_id_to_value[dict_id as usize];
        }
        Ok(())
    }

    fn get_dict_ids_in_range(
        &self,
        lower: Option<&DictionaryValue>,
        upper: Option<&DictionaryValue>,
        include_lower: bool,
        include_upper: bool,
    ) -> HashSet<i32> {
        let lower_val = lower.and_then(|v| v.as_long());
        let upper_val = upper.and_then(|v| v.as_long());

        let mut result = HashSet::new();

        let start_idx = match lower_val {
            Some(lower) => {
                let idx = self.dict_id_to_value.partition_point(|&x| x < lower);
                if include_lower {
                    idx
                } else {
                    self.dict_id_to_value.partition_point(|&x| x <= lower)
                }
            }
            None => 0,
        };

        let end_idx = match upper_val {
            Some(upper) => {
                if include_upper {
                    self.dict_id_to_value.partition_point(|&x| x <= upper)
                } else {
                    self.dict_id_to_value.partition_point(|&x| x < upper)
                }
            }
            None => self.dict_id_to_value.len(),
        };

        for dict_id in start_idx..end_idx {
            result.insert(dict_id as i32);
        }

        result
    }

    fn min_value(&self) -> Option<DictionaryValue> {
        self.min.map(DictionaryValue::Long)
    }

    fn max_value(&self) -> Option<DictionaryValue> {
        self.max.map(DictionaryValue::Long)
    }
}

/// On-heap immutable dictionary for DOUBLE values
#[derive(Debug)]
pub struct OnHeapDoubleDictionary {
    dict_id_to_value: Vec<f64>,
    value_to_dict_id: AHashMap<OrderedFloat<f64>, i32>,
    min: Option<f64>,
    max: Option<f64>,
}

impl OnHeapDoubleDictionary {
    pub fn new(values: Vec<f64>) -> Self {
        let len = values.len();
        let mut value_to_dict_id = AHashMap::with_capacity(len);

        for (dict_id, &value) in values.iter().enumerate() {
            value_to_dict_id.insert(OrderedFloat(value), dict_id as i32);
        }

        let min = values.first().copied();
        let max = values.last().copied();

        Self {
            dict_id_to_value: values,
            value_to_dict_id,
            min,
            max,
        }
    }

    pub fn from_unsorted(mut values: Vec<f64>) -> Self {
        values.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        // Remove duplicates (need special handling for floats)
        values.dedup_by(|a, b| (*a - *b).abs() < f64::EPSILON);
        Self::new(values)
    }
}

impl Dictionary for OnHeapDoubleDictionary {
    fn is_sorted(&self) -> bool {
        true
    }

    fn value_type(&self) -> DataType {
        DataType::Double
    }

    fn len(&self) -> usize {
        self.dict_id_to_value.len()
    }

    fn index_of(&self, value: &DictionaryValue) -> i32 {
        match value {
            DictionaryValue::Double(v) => self.index_of_double(v.0),
            DictionaryValue::Float(v) => self.index_of_double(v.0 as f64),
            DictionaryValue::Int(v) => self.index_of_double(*v as f64),
            DictionaryValue::Long(v) => self.index_of_double(*v as f64),
            _ => NULL_VALUE_INDEX,
        }
    }

    fn index_of_double(&self, value: f64) -> i32 {
        self.value_to_dict_id
            .get(&OrderedFloat(value))
            .copied()
            .unwrap_or(NULL_VALUE_INDEX)
    }

    fn get(&self, dict_id: i32) -> DictionaryResult<DictionaryValue> {
        self.get_double(dict_id).map(DictionaryValue::double)
    }

    fn get_double(&self, dict_id: i32) -> DictionaryResult<f64> {
        if dict_id < 0 || dict_id as usize >= self.dict_id_to_value.len() {
            return Err(DictionaryError::InvalidDictId { dict_id });
        }
        Ok(self.dict_id_to_value[dict_id as usize])
    }

    fn read_double_values(&self, dict_ids: &[i32], out: &mut [f64]) -> DictionaryResult<()> {
        for (i, &dict_id) in dict_ids.iter().enumerate() {
            if dict_id < 0 || dict_id as usize >= self.dict_id_to_value.len() {
                return Err(DictionaryError::InvalidDictId { dict_id });
            }
            out[i] = self.dict_id_to_value[dict_id as usize];
        }
        Ok(())
    }

    fn get_dict_ids_in_range(
        &self,
        lower: Option<&DictionaryValue>,
        upper: Option<&DictionaryValue>,
        include_lower: bool,
        include_upper: bool,
    ) -> HashSet<i32> {
        let lower_val = lower.and_then(|v| v.as_double());
        let upper_val = upper.and_then(|v| v.as_double());

        let mut result = HashSet::new();

        let start_idx = match lower_val {
            Some(lower) => {
                let idx = self
                    .dict_id_to_value
                    .partition_point(|&x| x < lower);
                if include_lower {
                    idx
                } else {
                    self.dict_id_to_value
                        .partition_point(|&x| x <= lower)
                }
            }
            None => 0,
        };

        let end_idx = match upper_val {
            Some(upper) => {
                if include_upper {
                    self.dict_id_to_value
                        .partition_point(|&x| x <= upper)
                } else {
                    self.dict_id_to_value.partition_point(|&x| x < upper)
                }
            }
            None => self.dict_id_to_value.len(),
        };

        for dict_id in start_idx..end_idx {
            result.insert(dict_id as i32);
        }

        result
    }

    fn min_value(&self) -> Option<DictionaryValue> {
        self.min.map(DictionaryValue::double)
    }

    fn max_value(&self) -> Option<DictionaryValue> {
        self.max.map(DictionaryValue::double)
    }
}

/// On-heap immutable dictionary for STRING values
#[derive(Debug)]
pub struct OnHeapStringDictionary {
    dict_id_to_value: Vec<String>,
    value_to_dict_id: AHashMap<String, i32>,
    min: Option<String>,
    max: Option<String>,
}

impl OnHeapStringDictionary {
    pub fn new(values: Vec<String>) -> Self {
        let len = values.len();
        let mut value_to_dict_id = AHashMap::with_capacity(len);

        for (dict_id, value) in values.iter().enumerate() {
            value_to_dict_id.insert(value.clone(), dict_id as i32);
        }

        let min = values.first().cloned();
        let max = values.last().cloned();

        Self {
            dict_id_to_value: values,
            value_to_dict_id,
            min,
            max,
        }
    }

    pub fn from_unsorted(mut values: Vec<String>) -> Self {
        values.sort();
        values.dedup();
        Self::new(values)
    }
}

impl Dictionary for OnHeapStringDictionary {
    fn is_sorted(&self) -> bool {
        true
    }

    fn value_type(&self) -> DataType {
        DataType::String
    }

    fn len(&self) -> usize {
        self.dict_id_to_value.len()
    }

    fn index_of(&self, value: &DictionaryValue) -> i32 {
        match value {
            DictionaryValue::String(v) => self.index_of_string(v),
            _ => self.index_of_string(&value.as_string()),
        }
    }

    fn index_of_string(&self, value: &str) -> i32 {
        self.value_to_dict_id
            .get(value)
            .copied()
            .unwrap_or(NULL_VALUE_INDEX)
    }

    fn get(&self, dict_id: i32) -> DictionaryResult<DictionaryValue> {
        self.get_string(dict_id).map(DictionaryValue::String)
    }

    fn get_string(&self, dict_id: i32) -> DictionaryResult<String> {
        if dict_id < 0 || dict_id as usize >= self.dict_id_to_value.len() {
            return Err(DictionaryError::InvalidDictId { dict_id });
        }
        Ok(self.dict_id_to_value[dict_id as usize].clone())
    }

    fn get_dict_ids_in_range(
        &self,
        lower: Option<&DictionaryValue>,
        upper: Option<&DictionaryValue>,
        include_lower: bool,
        include_upper: bool,
    ) -> HashSet<i32> {
        let lower_str = lower.map(|v| v.as_string());
        let upper_str = upper.map(|v| v.as_string());

        let mut result = HashSet::new();

        let start_idx = match &lower_str {
            Some(lower) => {
                let idx = self
                    .dict_id_to_value
                    .partition_point(|x| x.as_str() < lower.as_str());
                if include_lower {
                    idx
                } else {
                    self.dict_id_to_value
                        .partition_point(|x| x.as_str() <= lower.as_str())
                }
            }
            None => 0,
        };

        let end_idx = match &upper_str {
            Some(upper) => {
                if include_upper {
                    self.dict_id_to_value
                        .partition_point(|x| x.as_str() <= upper.as_str())
                } else {
                    self.dict_id_to_value
                        .partition_point(|x| x.as_str() < upper.as_str())
                }
            }
            None => self.dict_id_to_value.len(),
        };

        for dict_id in start_idx..end_idx {
            result.insert(dict_id as i32);
        }

        result
    }

    fn min_value(&self) -> Option<DictionaryValue> {
        self.min.clone().map(DictionaryValue::String)
    }

    fn max_value(&self) -> Option<DictionaryValue> {
        self.max.clone().map(DictionaryValue::String)
    }

    fn insertion_index_of(&self, value: &DictionaryValue) -> i32 {
        let str_val = value.as_string();
        match self.dict_id_to_value.binary_search(&str_val) {
            Ok(idx) => idx as i32,
            Err(idx) => -(idx as i32 + 1),
        }
    }
}

/// On-heap immutable dictionary for BYTES values
#[derive(Debug)]
pub struct OnHeapBytesDictionary {
    dict_id_to_value: Vec<Vec<u8>>,
    value_to_dict_id: AHashMap<Vec<u8>, i32>,
    min: Option<Vec<u8>>,
    max: Option<Vec<u8>>,
}

impl OnHeapBytesDictionary {
    pub fn new(values: Vec<Vec<u8>>) -> Self {
        let len = values.len();
        let mut value_to_dict_id = AHashMap::with_capacity(len);

        for (dict_id, value) in values.iter().enumerate() {
            value_to_dict_id.insert(value.clone(), dict_id as i32);
        }

        let min = values.first().cloned();
        let max = values.last().cloned();

        Self {
            dict_id_to_value: values,
            value_to_dict_id,
            min,
            max,
        }
    }

    pub fn from_unsorted(mut values: Vec<Vec<u8>>) -> Self {
        values.sort();
        values.dedup();
        Self::new(values)
    }
}

impl Dictionary for OnHeapBytesDictionary {
    fn is_sorted(&self) -> bool {
        true
    }

    fn value_type(&self) -> DataType {
        DataType::Bytes
    }

    fn len(&self) -> usize {
        self.dict_id_to_value.len()
    }

    fn index_of(&self, value: &DictionaryValue) -> i32 {
        match value {
            DictionaryValue::Bytes(v) => self.index_of_bytes(v),
            _ => NULL_VALUE_INDEX,
        }
    }

    fn index_of_bytes(&self, value: &[u8]) -> i32 {
        self.value_to_dict_id
            .get(value)
            .copied()
            .unwrap_or(NULL_VALUE_INDEX)
    }

    fn get(&self, dict_id: i32) -> DictionaryResult<DictionaryValue> {
        self.get_bytes(dict_id).map(DictionaryValue::Bytes)
    }

    fn get_bytes(&self, dict_id: i32) -> DictionaryResult<Vec<u8>> {
        if dict_id < 0 || dict_id as usize >= self.dict_id_to_value.len() {
            return Err(DictionaryError::InvalidDictId { dict_id });
        }
        Ok(self.dict_id_to_value[dict_id as usize].clone())
    }

    fn get_dict_ids_in_range(
        &self,
        lower: Option<&DictionaryValue>,
        upper: Option<&DictionaryValue>,
        include_lower: bool,
        include_upper: bool,
    ) -> HashSet<i32> {
        let lower_bytes = lower.map(|v| v.as_bytes());
        let upper_bytes = upper.map(|v| v.as_bytes());

        let mut result = HashSet::new();

        let start_idx = match &lower_bytes {
            Some(lower) => {
                let idx = self
                    .dict_id_to_value
                    .partition_point(|x| x.as_slice() < lower.as_slice());
                if include_lower {
                    idx
                } else {
                    self.dict_id_to_value
                        .partition_point(|x| x.as_slice() <= lower.as_slice())
                }
            }
            None => 0,
        };

        let end_idx = match &upper_bytes {
            Some(upper) => {
                if include_upper {
                    self.dict_id_to_value
                        .partition_point(|x| x.as_slice() <= upper.as_slice())
                } else {
                    self.dict_id_to_value
                        .partition_point(|x| x.as_slice() < upper.as_slice())
                }
            }
            None => self.dict_id_to_value.len(),
        };

        for dict_id in start_idx..end_idx {
            result.insert(dict_id as i32);
        }

        result
    }

    fn min_value(&self) -> Option<DictionaryValue> {
        self.min.clone().map(DictionaryValue::Bytes)
    }

    fn max_value(&self) -> Option<DictionaryValue> {
        self.max.clone().map(DictionaryValue::Bytes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_int_dictionary() {
        let dict = OnHeapIntDictionary::new(vec![10, 20, 30, 40, 50]);

        // Test basic properties
        assert!(dict.is_sorted());
        assert_eq!(dict.value_type(), DataType::Int);
        assert_eq!(dict.len(), 5);

        // Test encoding (value -> dict_id)
        assert_eq!(dict.index_of_int(10), 0);
        assert_eq!(dict.index_of_int(30), 2);
        assert_eq!(dict.index_of_int(50), 4);
        assert_eq!(dict.index_of_int(25), NULL_VALUE_INDEX);

        // Test decoding (dict_id -> value)
        assert_eq!(dict.get_int(0).unwrap(), 10);
        assert_eq!(dict.get_int(2).unwrap(), 30);
        assert_eq!(dict.get_int(4).unwrap(), 50);

        // Test min/max
        assert_eq!(dict.min_value(), Some(DictionaryValue::Int(10)));
        assert_eq!(dict.max_value(), Some(DictionaryValue::Int(50)));

        // Test range query
        let in_range = dict.get_dict_ids_in_range(
            Some(&DictionaryValue::Int(20)),
            Some(&DictionaryValue::Int(40)),
            true,
            true,
        );
        assert_eq!(in_range.len(), 3);
        assert!(in_range.contains(&1)); // 20
        assert!(in_range.contains(&2)); // 30
        assert!(in_range.contains(&3)); // 40

        // Test range query exclusive
        let in_range_excl = dict.get_dict_ids_in_range(
            Some(&DictionaryValue::Int(20)),
            Some(&DictionaryValue::Int(40)),
            false,
            false,
        );
        assert_eq!(in_range_excl.len(), 1);
        assert!(in_range_excl.contains(&2)); // 30

        // Test binary search
        assert_eq!(dict.insertion_index_of(&DictionaryValue::Int(30)), 2);
        assert_eq!(dict.insertion_index_of(&DictionaryValue::Int(25)), -3); // -(2+1)
    }

    #[test]
    fn test_int_dictionary_from_unsorted() {
        let dict = OnHeapIntDictionary::from_unsorted(vec![50, 10, 30, 20, 40, 30]); // 30 duplicated

        assert_eq!(dict.len(), 5); // Duplicates removed
        assert_eq!(dict.get_int(0).unwrap(), 10);
        assert_eq!(dict.get_int(4).unwrap(), 50);
    }

    #[test]
    fn test_long_dictionary() {
        let dict = OnHeapLongDictionary::new(vec![100i64, 200, 300]);

        assert_eq!(dict.index_of_long(200), 1);
        assert_eq!(dict.get_long(1).unwrap(), 200);
    }

    #[test]
    fn test_double_dictionary() {
        let dict = OnHeapDoubleDictionary::new(vec![1.0, 2.5, 3.14, 4.0]);

        assert_eq!(dict.index_of_double(3.14), 2);
        assert!((dict.get_double(2).unwrap() - 3.14).abs() < f64::EPSILON);
    }

    #[test]
    fn test_string_dictionary() {
        let dict = OnHeapStringDictionary::new(vec![
            "apple".to_string(),
            "banana".to_string(),
            "cherry".to_string(),
        ]);

        assert_eq!(dict.index_of_string("banana"), 1);
        assert_eq!(dict.get_string(1).unwrap(), "banana");
        assert_eq!(dict.index_of_string("date"), NULL_VALUE_INDEX);

        // Test range query on strings
        let in_range = dict.get_dict_ids_in_range(
            Some(&DictionaryValue::String("apple".to_string())),
            Some(&DictionaryValue::String("cherry".to_string())),
            true,
            false,
        );
        assert_eq!(in_range.len(), 2);
        assert!(in_range.contains(&0)); // apple
        assert!(in_range.contains(&1)); // banana
    }

    #[test]
    fn test_bytes_dictionary() {
        let dict = OnHeapBytesDictionary::new(vec![
            vec![0x01, 0x02],
            vec![0x03, 0x04],
            vec![0x05, 0x06],
        ]);

        assert_eq!(dict.index_of_bytes(&[0x03, 0x04]), 1);
        assert_eq!(dict.get_bytes(1).unwrap(), vec![0x03, 0x04]);
    }

    #[test]
    fn test_batch_read() {
        let dict = OnHeapIntDictionary::new(vec![100, 200, 300, 400, 500]);
        let dict_ids = [0, 2, 4];
        let mut out = [0i32; 3];

        dict.read_int_values(&dict_ids, &mut out).unwrap();
        assert_eq!(out, [100, 300, 500]);
    }

    #[test]
    fn test_empty_dictionary() {
        let dict = OnHeapIntDictionary::new(vec![]);

        assert!(dict.is_empty());
        assert_eq!(dict.len(), 0);
        assert_eq!(dict.index_of_int(1), NULL_VALUE_INDEX);
        assert!(dict.get_int(0).is_err());
        assert_eq!(dict.min_value(), None);
        assert_eq!(dict.max_value(), None);
    }

    #[test]
    fn test_single_element_dictionary() {
        let dict = OnHeapIntDictionary::new(vec![42]);

        assert_eq!(dict.len(), 1);
        assert_eq!(dict.index_of_int(42), 0);
        assert_eq!(dict.get_int(0).unwrap(), 42);
        assert_eq!(dict.min_value(), Some(DictionaryValue::Int(42)));
        assert_eq!(dict.max_value(), Some(DictionaryValue::Int(42)));
    }

    #[test]
    fn test_type_coercion() {
        let dict = OnHeapDoubleDictionary::new(vec![1.0, 2.0, 3.0]);

        // Int value should be coerced to double
        assert_eq!(dict.index_of(&DictionaryValue::Int(2)), 1);
    }
}
