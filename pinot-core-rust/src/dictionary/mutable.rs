//! Mutable dictionary implementations for realtime segments
//!
//! These dictionaries support dynamic insertion of new values while
//! maintaining thread-safe read access.

use super::{
    DataType, Dictionary, DictionaryError, DictionaryResult, DictionaryValue, MutableDictionary,
    NULL_VALUE_INDEX,
};
use ahash::AHashMap;
use ordered_float::OrderedFloat;
use parking_lot::RwLock;
use std::collections::HashSet;
use std::sync::atomic::{AtomicI32, AtomicUsize, Ordering};

/// Segment size for 2D array storage (8192 entries per segment)
const SEGMENT_SHIFT: usize = 13;
const SEGMENT_SIZE: usize = 1 << SEGMENT_SHIFT;
const SEGMENT_MASK: usize = SEGMENT_SIZE - 1;

/// Thread-safe mutable dictionary for INT values
pub struct IntOnHeapMutableDictionary {
    /// Value -> Dict ID mapping
    value_to_dict_id: RwLock<AHashMap<i32, i32>>,
    /// Dict ID -> Value mapping using 2D array for efficient growth
    dict_id_to_value: RwLock<Vec<Vec<i32>>>,
    /// Number of entries
    entries_indexed: AtomicUsize,
    /// Minimum value
    min: AtomicI32,
    /// Maximum value
    max: AtomicI32,
    /// Whether min/max have been initialized
    initialized: std::sync::atomic::AtomicBool,
}

impl IntOnHeapMutableDictionary {
    pub fn new() -> Self {
        Self {
            value_to_dict_id: RwLock::new(AHashMap::new()),
            dict_id_to_value: RwLock::new(vec![Vec::with_capacity(SEGMENT_SIZE)]),
            entries_indexed: AtomicUsize::new(0),
            min: AtomicI32::new(i32::MAX),
            max: AtomicI32::new(i32::MIN),
            initialized: std::sync::atomic::AtomicBool::new(false),
        }
    }

    fn update_min_max(&self, value: i32) {
        // Atomically update min
        let mut current_min = self.min.load(Ordering::Relaxed);
        while value < current_min {
            match self.min.compare_exchange_weak(
                current_min,
                value,
                Ordering::Release,
                Ordering::Relaxed,
            ) {
                Ok(_) => break,
                Err(x) => current_min = x,
            }
        }

        // Atomically update max
        let mut current_max = self.max.load(Ordering::Relaxed);
        while value > current_max {
            match self.max.compare_exchange_weak(
                current_max,
                value,
                Ordering::Release,
                Ordering::Relaxed,
            ) {
                Ok(_) => break,
                Err(x) => current_max = x,
            }
        }

        self.initialized.store(true, Ordering::Release);
    }

    fn index_value(&self, value: i32) -> i32 {
        // First check if value exists (read lock)
        {
            let map = self.value_to_dict_id.read();
            if let Some(&dict_id) = map.get(&value) {
                return dict_id;
            }
        }

        // Value doesn't exist, need to add it (write lock)
        let mut map = self.value_to_dict_id.write();

        // Double-check after acquiring write lock
        if let Some(&dict_id) = map.get(&value) {
            return dict_id;
        }

        // Add new value
        let new_dict_id = self.entries_indexed.fetch_add(1, Ordering::SeqCst) as i32;
        let segment_idx = (new_dict_id as usize) >> SEGMENT_SHIFT;
        let offset = (new_dict_id as usize) & SEGMENT_MASK;

        // Grow storage if needed
        {
            let mut storage = self.dict_id_to_value.write();
            while storage.len() <= segment_idx {
                storage.push(Vec::with_capacity(SEGMENT_SIZE));
            }
            // Ensure the segment has enough capacity
            while storage[segment_idx].len() <= offset {
                storage[segment_idx].push(0);
            }
            storage[segment_idx][offset] = value;
        }

        map.insert(value, new_dict_id);
        self.update_min_max(value);

        new_dict_id
    }
}

impl Default for IntOnHeapMutableDictionary {
    fn default() -> Self {
        Self::new()
    }
}

impl Dictionary for IntOnHeapMutableDictionary {
    fn is_sorted(&self) -> bool {
        false
    }

    fn value_type(&self) -> DataType {
        DataType::Int
    }

    fn len(&self) -> usize {
        self.entries_indexed.load(Ordering::Acquire)
    }

    fn index_of(&self, value: &DictionaryValue) -> i32 {
        match value {
            DictionaryValue::Int(v) => self.index_of_int(*v),
            _ => NULL_VALUE_INDEX,
        }
    }

    fn index_of_int(&self, value: i32) -> i32 {
        let map = self.value_to_dict_id.read();
        map.get(&value).copied().unwrap_or(NULL_VALUE_INDEX)
    }

    fn get(&self, dict_id: i32) -> DictionaryResult<DictionaryValue> {
        self.get_int(dict_id).map(DictionaryValue::Int)
    }

    fn get_int(&self, dict_id: i32) -> DictionaryResult<i32> {
        if dict_id < 0 {
            return Err(DictionaryError::InvalidDictId { dict_id });
        }
        let dict_id = dict_id as usize;
        let segment_idx = dict_id >> SEGMENT_SHIFT;
        let offset = dict_id & SEGMENT_MASK;

        let storage = self.dict_id_to_value.read();
        if segment_idx >= storage.len() || offset >= storage[segment_idx].len() {
            return Err(DictionaryError::InvalidDictId {
                dict_id: dict_id as i32,
            });
        }

        Ok(storage[segment_idx][offset])
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
        let storage = self.dict_id_to_value.read();
        let num_entries = self.entries_indexed.load(Ordering::Acquire);

        for dict_id in 0..num_entries {
            let segment_idx = dict_id >> SEGMENT_SHIFT;
            let offset = dict_id & SEGMENT_MASK;

            if segment_idx >= storage.len() || offset >= storage[segment_idx].len() {
                continue;
            }

            let value = storage[segment_idx][offset];

            let lower_ok = match lower_val {
                Some(l) if include_lower => value >= l,
                Some(l) => value > l,
                None => true,
            };

            let upper_ok = match upper_val {
                Some(u) if include_upper => value <= u,
                Some(u) => value < u,
                None => true,
            };

            if lower_ok && upper_ok {
                result.insert(dict_id as i32);
            }
        }

        result
    }

    fn min_value(&self) -> Option<DictionaryValue> {
        if self.initialized.load(Ordering::Acquire) {
            Some(DictionaryValue::Int(self.min.load(Ordering::Acquire)))
        } else {
            None
        }
    }

    fn max_value(&self) -> Option<DictionaryValue> {
        if self.initialized.load(Ordering::Acquire) {
            Some(DictionaryValue::Int(self.max.load(Ordering::Acquire)))
        } else {
            None
        }
    }
}

impl MutableDictionary for IntOnHeapMutableDictionary {
    fn index(&mut self, value: DictionaryValue) -> DictionaryResult<i32> {
        match value {
            DictionaryValue::Int(v) => Ok(self.index_value(v)),
            DictionaryValue::Long(v) => Ok(self.index_value(v as i32)),
            _ => Err(DictionaryError::TypeMismatch {
                expected: "Int".to_string(),
                actual: format!("{:?}", value.data_type()),
            }),
        }
    }
}

// Thread-safe implementation markers
unsafe impl Send for IntOnHeapMutableDictionary {}
unsafe impl Sync for IntOnHeapMutableDictionary {}

/// Thread-safe mutable dictionary for LONG values
pub struct LongOnHeapMutableDictionary {
    value_to_dict_id: RwLock<AHashMap<i64, i32>>,
    dict_id_to_value: RwLock<Vec<Vec<i64>>>,
    entries_indexed: AtomicUsize,
    min: std::sync::atomic::AtomicI64,
    max: std::sync::atomic::AtomicI64,
    initialized: std::sync::atomic::AtomicBool,
}

impl LongOnHeapMutableDictionary {
    pub fn new() -> Self {
        Self {
            value_to_dict_id: RwLock::new(AHashMap::new()),
            dict_id_to_value: RwLock::new(vec![Vec::with_capacity(SEGMENT_SIZE)]),
            entries_indexed: AtomicUsize::new(0),
            min: std::sync::atomic::AtomicI64::new(i64::MAX),
            max: std::sync::atomic::AtomicI64::new(i64::MIN),
            initialized: std::sync::atomic::AtomicBool::new(false),
        }
    }

    fn update_min_max(&self, value: i64) {
        let mut current_min = self.min.load(Ordering::Relaxed);
        while value < current_min {
            match self.min.compare_exchange_weak(
                current_min,
                value,
                Ordering::Release,
                Ordering::Relaxed,
            ) {
                Ok(_) => break,
                Err(x) => current_min = x,
            }
        }

        let mut current_max = self.max.load(Ordering::Relaxed);
        while value > current_max {
            match self.max.compare_exchange_weak(
                current_max,
                value,
                Ordering::Release,
                Ordering::Relaxed,
            ) {
                Ok(_) => break,
                Err(x) => current_max = x,
            }
        }

        self.initialized.store(true, Ordering::Release);
    }

    fn index_value(&self, value: i64) -> i32 {
        {
            let map = self.value_to_dict_id.read();
            if let Some(&dict_id) = map.get(&value) {
                return dict_id;
            }
        }

        let mut map = self.value_to_dict_id.write();
        if let Some(&dict_id) = map.get(&value) {
            return dict_id;
        }

        let new_dict_id = self.entries_indexed.fetch_add(1, Ordering::SeqCst) as i32;
        let segment_idx = (new_dict_id as usize) >> SEGMENT_SHIFT;
        let offset = (new_dict_id as usize) & SEGMENT_MASK;

        {
            let mut storage = self.dict_id_to_value.write();
            while storage.len() <= segment_idx {
                storage.push(Vec::with_capacity(SEGMENT_SIZE));
            }
            while storage[segment_idx].len() <= offset {
                storage[segment_idx].push(0);
            }
            storage[segment_idx][offset] = value;
        }

        map.insert(value, new_dict_id);
        self.update_min_max(value);

        new_dict_id
    }
}

impl Default for LongOnHeapMutableDictionary {
    fn default() -> Self {
        Self::new()
    }
}

impl Dictionary for LongOnHeapMutableDictionary {
    fn is_sorted(&self) -> bool {
        false
    }

    fn value_type(&self) -> DataType {
        DataType::Long
    }

    fn len(&self) -> usize {
        self.entries_indexed.load(Ordering::Acquire)
    }

    fn index_of(&self, value: &DictionaryValue) -> i32 {
        match value {
            DictionaryValue::Long(v) => self.index_of_long(*v),
            DictionaryValue::Int(v) => self.index_of_long(*v as i64),
            _ => NULL_VALUE_INDEX,
        }
    }

    fn index_of_long(&self, value: i64) -> i32 {
        let map = self.value_to_dict_id.read();
        map.get(&value).copied().unwrap_or(NULL_VALUE_INDEX)
    }

    fn get(&self, dict_id: i32) -> DictionaryResult<DictionaryValue> {
        self.get_long(dict_id).map(DictionaryValue::Long)
    }

    fn get_long(&self, dict_id: i32) -> DictionaryResult<i64> {
        if dict_id < 0 {
            return Err(DictionaryError::InvalidDictId { dict_id });
        }
        let dict_id = dict_id as usize;
        let segment_idx = dict_id >> SEGMENT_SHIFT;
        let offset = dict_id & SEGMENT_MASK;

        let storage = self.dict_id_to_value.read();
        if segment_idx >= storage.len() || offset >= storage[segment_idx].len() {
            return Err(DictionaryError::InvalidDictId {
                dict_id: dict_id as i32,
            });
        }

        Ok(storage[segment_idx][offset])
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
        let storage = self.dict_id_to_value.read();
        let num_entries = self.entries_indexed.load(Ordering::Acquire);

        for dict_id in 0..num_entries {
            let segment_idx = dict_id >> SEGMENT_SHIFT;
            let offset = dict_id & SEGMENT_MASK;

            if segment_idx >= storage.len() || offset >= storage[segment_idx].len() {
                continue;
            }

            let value = storage[segment_idx][offset];

            let lower_ok = match lower_val {
                Some(l) if include_lower => value >= l,
                Some(l) => value > l,
                None => true,
            };

            let upper_ok = match upper_val {
                Some(u) if include_upper => value <= u,
                Some(u) => value < u,
                None => true,
            };

            if lower_ok && upper_ok {
                result.insert(dict_id as i32);
            }
        }

        result
    }

    fn min_value(&self) -> Option<DictionaryValue> {
        if self.initialized.load(Ordering::Acquire) {
            Some(DictionaryValue::Long(self.min.load(Ordering::Acquire)))
        } else {
            None
        }
    }

    fn max_value(&self) -> Option<DictionaryValue> {
        if self.initialized.load(Ordering::Acquire) {
            Some(DictionaryValue::Long(self.max.load(Ordering::Acquire)))
        } else {
            None
        }
    }
}

impl MutableDictionary for LongOnHeapMutableDictionary {
    fn index(&mut self, value: DictionaryValue) -> DictionaryResult<i32> {
        match value {
            DictionaryValue::Long(v) => Ok(self.index_value(v)),
            DictionaryValue::Int(v) => Ok(self.index_value(v as i64)),
            _ => Err(DictionaryError::TypeMismatch {
                expected: "Long".to_string(),
                actual: format!("{:?}", value.data_type()),
            }),
        }
    }
}

unsafe impl Send for LongOnHeapMutableDictionary {}
unsafe impl Sync for LongOnHeapMutableDictionary {}

/// Thread-safe mutable dictionary for STRING values
pub struct StringOnHeapMutableDictionary {
    value_to_dict_id: RwLock<AHashMap<String, i32>>,
    dict_id_to_value: RwLock<Vec<Vec<String>>>,
    entries_indexed: AtomicUsize,
    min: RwLock<Option<String>>,
    max: RwLock<Option<String>>,
}

impl StringOnHeapMutableDictionary {
    pub fn new() -> Self {
        Self {
            value_to_dict_id: RwLock::new(AHashMap::new()),
            dict_id_to_value: RwLock::new(vec![Vec::with_capacity(SEGMENT_SIZE)]),
            entries_indexed: AtomicUsize::new(0),
            min: RwLock::new(None),
            max: RwLock::new(None),
        }
    }

    fn update_min_max(&self, value: &str) {
        let mut min = self.min.write();
        match &*min {
            None => *min = Some(value.to_string()),
            Some(current_min) if value < current_min.as_str() => *min = Some(value.to_string()),
            _ => {}
        }

        let mut max = self.max.write();
        match &*max {
            None => *max = Some(value.to_string()),
            Some(current_max) if value > current_max.as_str() => *max = Some(value.to_string()),
            _ => {}
        }
    }

    fn index_value(&self, value: String) -> i32 {
        {
            let map = self.value_to_dict_id.read();
            if let Some(&dict_id) = map.get(&value) {
                return dict_id;
            }
        }

        let mut map = self.value_to_dict_id.write();
        if let Some(&dict_id) = map.get(&value) {
            return dict_id;
        }

        let new_dict_id = self.entries_indexed.fetch_add(1, Ordering::SeqCst) as i32;
        let segment_idx = (new_dict_id as usize) >> SEGMENT_SHIFT;
        let offset = (new_dict_id as usize) & SEGMENT_MASK;

        {
            let mut storage = self.dict_id_to_value.write();
            while storage.len() <= segment_idx {
                storage.push(Vec::with_capacity(SEGMENT_SIZE));
            }
            while storage[segment_idx].len() <= offset {
                storage[segment_idx].push(String::new());
            }
            storage[segment_idx][offset] = value.clone();
        }

        map.insert(value.clone(), new_dict_id);
        self.update_min_max(&value);

        new_dict_id
    }
}

impl Default for StringOnHeapMutableDictionary {
    fn default() -> Self {
        Self::new()
    }
}

impl Dictionary for StringOnHeapMutableDictionary {
    fn is_sorted(&self) -> bool {
        false
    }

    fn value_type(&self) -> DataType {
        DataType::String
    }

    fn len(&self) -> usize {
        self.entries_indexed.load(Ordering::Acquire)
    }

    fn index_of(&self, value: &DictionaryValue) -> i32 {
        match value {
            DictionaryValue::String(v) => self.index_of_string(v),
            _ => self.index_of_string(&value.as_string()),
        }
    }

    fn index_of_string(&self, value: &str) -> i32 {
        let map = self.value_to_dict_id.read();
        map.get(value).copied().unwrap_or(NULL_VALUE_INDEX)
    }

    fn get(&self, dict_id: i32) -> DictionaryResult<DictionaryValue> {
        self.get_string(dict_id).map(DictionaryValue::String)
    }

    fn get_string(&self, dict_id: i32) -> DictionaryResult<String> {
        if dict_id < 0 {
            return Err(DictionaryError::InvalidDictId { dict_id });
        }
        let dict_id = dict_id as usize;
        let segment_idx = dict_id >> SEGMENT_SHIFT;
        let offset = dict_id & SEGMENT_MASK;

        let storage = self.dict_id_to_value.read();
        if segment_idx >= storage.len() || offset >= storage[segment_idx].len() {
            return Err(DictionaryError::InvalidDictId {
                dict_id: dict_id as i32,
            });
        }

        Ok(storage[segment_idx][offset].clone())
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
        let storage = self.dict_id_to_value.read();
        let num_entries = self.entries_indexed.load(Ordering::Acquire);

        for dict_id in 0..num_entries {
            let segment_idx = dict_id >> SEGMENT_SHIFT;
            let offset = dict_id & SEGMENT_MASK;

            if segment_idx >= storage.len() || offset >= storage[segment_idx].len() {
                continue;
            }

            let value = &storage[segment_idx][offset];

            let lower_ok = match &lower_str {
                Some(l) if include_lower => value.as_str() >= l.as_str(),
                Some(l) => value.as_str() > l.as_str(),
                None => true,
            };

            let upper_ok = match &upper_str {
                Some(u) if include_upper => value.as_str() <= u.as_str(),
                Some(u) => value.as_str() < u.as_str(),
                None => true,
            };

            if lower_ok && upper_ok {
                result.insert(dict_id as i32);
            }
        }

        result
    }

    fn min_value(&self) -> Option<DictionaryValue> {
        self.min.read().clone().map(DictionaryValue::String)
    }

    fn max_value(&self) -> Option<DictionaryValue> {
        self.max.read().clone().map(DictionaryValue::String)
    }
}

impl MutableDictionary for StringOnHeapMutableDictionary {
    fn index(&mut self, value: DictionaryValue) -> DictionaryResult<i32> {
        match value {
            DictionaryValue::String(v) => Ok(self.index_value(v)),
            _ => Ok(self.index_value(value.as_string())),
        }
    }
}

unsafe impl Send for StringOnHeapMutableDictionary {}
unsafe impl Sync for StringOnHeapMutableDictionary {}

/// Thread-safe mutable dictionary for DOUBLE values
pub struct DoubleOnHeapMutableDictionary {
    value_to_dict_id: RwLock<AHashMap<OrderedFloat<f64>, i32>>,
    dict_id_to_value: RwLock<Vec<Vec<f64>>>,
    entries_indexed: AtomicUsize,
    min: RwLock<Option<f64>>,
    max: RwLock<Option<f64>>,
}

impl DoubleOnHeapMutableDictionary {
    pub fn new() -> Self {
        Self {
            value_to_dict_id: RwLock::new(AHashMap::new()),
            dict_id_to_value: RwLock::new(vec![Vec::with_capacity(SEGMENT_SIZE)]),
            entries_indexed: AtomicUsize::new(0),
            min: RwLock::new(None),
            max: RwLock::new(None),
        }
    }

    fn update_min_max(&self, value: f64) {
        let mut min = self.min.write();
        match *min {
            None => *min = Some(value),
            Some(current_min) if value < current_min => *min = Some(value),
            _ => {}
        }

        let mut max = self.max.write();
        match *max {
            None => *max = Some(value),
            Some(current_max) if value > current_max => *max = Some(value),
            _ => {}
        }
    }

    fn index_value(&self, value: f64) -> i32 {
        let key = OrderedFloat(value);

        {
            let map = self.value_to_dict_id.read();
            if let Some(&dict_id) = map.get(&key) {
                return dict_id;
            }
        }

        let mut map = self.value_to_dict_id.write();
        if let Some(&dict_id) = map.get(&key) {
            return dict_id;
        }

        let new_dict_id = self.entries_indexed.fetch_add(1, Ordering::SeqCst) as i32;
        let segment_idx = (new_dict_id as usize) >> SEGMENT_SHIFT;
        let offset = (new_dict_id as usize) & SEGMENT_MASK;

        {
            let mut storage = self.dict_id_to_value.write();
            while storage.len() <= segment_idx {
                storage.push(Vec::with_capacity(SEGMENT_SIZE));
            }
            while storage[segment_idx].len() <= offset {
                storage[segment_idx].push(0.0);
            }
            storage[segment_idx][offset] = value;
        }

        map.insert(key, new_dict_id);
        self.update_min_max(value);

        new_dict_id
    }
}

impl Default for DoubleOnHeapMutableDictionary {
    fn default() -> Self {
        Self::new()
    }
}

impl Dictionary for DoubleOnHeapMutableDictionary {
    fn is_sorted(&self) -> bool {
        false
    }

    fn value_type(&self) -> DataType {
        DataType::Double
    }

    fn len(&self) -> usize {
        self.entries_indexed.load(Ordering::Acquire)
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
        let map = self.value_to_dict_id.read();
        map.get(&OrderedFloat(value))
            .copied()
            .unwrap_or(NULL_VALUE_INDEX)
    }

    fn get(&self, dict_id: i32) -> DictionaryResult<DictionaryValue> {
        self.get_double(dict_id).map(DictionaryValue::double)
    }

    fn get_double(&self, dict_id: i32) -> DictionaryResult<f64> {
        if dict_id < 0 {
            return Err(DictionaryError::InvalidDictId { dict_id });
        }
        let dict_id = dict_id as usize;
        let segment_idx = dict_id >> SEGMENT_SHIFT;
        let offset = dict_id & SEGMENT_MASK;

        let storage = self.dict_id_to_value.read();
        if segment_idx >= storage.len() || offset >= storage[segment_idx].len() {
            return Err(DictionaryError::InvalidDictId {
                dict_id: dict_id as i32,
            });
        }

        Ok(storage[segment_idx][offset])
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
        let storage = self.dict_id_to_value.read();
        let num_entries = self.entries_indexed.load(Ordering::Acquire);

        for dict_id in 0..num_entries {
            let segment_idx = dict_id >> SEGMENT_SHIFT;
            let offset = dict_id & SEGMENT_MASK;

            if segment_idx >= storage.len() || offset >= storage[segment_idx].len() {
                continue;
            }

            let value = storage[segment_idx][offset];

            let lower_ok = match lower_val {
                Some(l) if include_lower => value >= l,
                Some(l) => value > l,
                None => true,
            };

            let upper_ok = match upper_val {
                Some(u) if include_upper => value <= u,
                Some(u) => value < u,
                None => true,
            };

            if lower_ok && upper_ok {
                result.insert(dict_id as i32);
            }
        }

        result
    }

    fn min_value(&self) -> Option<DictionaryValue> {
        self.min.read().map(DictionaryValue::double)
    }

    fn max_value(&self) -> Option<DictionaryValue> {
        self.max.read().map(DictionaryValue::double)
    }
}

impl MutableDictionary for DoubleOnHeapMutableDictionary {
    fn index(&mut self, value: DictionaryValue) -> DictionaryResult<i32> {
        match value {
            DictionaryValue::Double(v) => Ok(self.index_value(v.0)),
            DictionaryValue::Float(v) => Ok(self.index_value(v.0 as f64)),
            DictionaryValue::Int(v) => Ok(self.index_value(v as f64)),
            DictionaryValue::Long(v) => Ok(self.index_value(v as f64)),
            _ => Err(DictionaryError::TypeMismatch {
                expected: "Double".to_string(),
                actual: format!("{:?}", value.data_type()),
            }),
        }
    }
}

unsafe impl Send for DoubleOnHeapMutableDictionary {}
unsafe impl Sync for DoubleOnHeapMutableDictionary {}

/// Factory for creating mutable dictionaries
pub struct MutableDictionaryFactory;

impl MutableDictionaryFactory {
    pub fn create(data_type: DataType) -> Box<dyn MutableDictionary + Send + Sync> {
        match data_type {
            DataType::Int => Box::new(IntOnHeapMutableDictionary::new()),
            DataType::Long => Box::new(LongOnHeapMutableDictionary::new()),
            DataType::Double | DataType::Float => Box::new(DoubleOnHeapMutableDictionary::new()),
            DataType::String => Box::new(StringOnHeapMutableDictionary::new()),
            _ => panic!("Unsupported data type for mutable dictionary: {:?}", data_type),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_int_mutable_dictionary() {
        let mut dict = IntOnHeapMutableDictionary::new();

        // Index new values
        assert_eq!(dict.index(DictionaryValue::Int(100)).unwrap(), 0);
        assert_eq!(dict.index(DictionaryValue::Int(200)).unwrap(), 1);
        assert_eq!(dict.index(DictionaryValue::Int(300)).unwrap(), 2);

        // Re-indexing returns same ID
        assert_eq!(dict.index(DictionaryValue::Int(100)).unwrap(), 0);
        assert_eq!(dict.index(DictionaryValue::Int(200)).unwrap(), 1);

        // Test lookup
        assert_eq!(dict.index_of_int(100), 0);
        assert_eq!(dict.index_of_int(200), 1);
        assert_eq!(dict.index_of_int(400), NULL_VALUE_INDEX);

        // Test get
        assert_eq!(dict.get_int(0).unwrap(), 100);
        assert_eq!(dict.get_int(1).unwrap(), 200);
        assert_eq!(dict.get_int(2).unwrap(), 300);

        // Test length
        assert_eq!(dict.len(), 3);

        // Test min/max
        assert_eq!(dict.min_value(), Some(DictionaryValue::Int(100)));
        assert_eq!(dict.max_value(), Some(DictionaryValue::Int(300)));
    }

    #[test]
    fn test_string_mutable_dictionary() {
        let mut dict = StringOnHeapMutableDictionary::new();

        dict.index(DictionaryValue::String("hello".to_string()))
            .unwrap();
        dict.index(DictionaryValue::String("world".to_string()))
            .unwrap();
        dict.index(DictionaryValue::String("foo".to_string()))
            .unwrap();

        assert_eq!(dict.index_of_string("hello"), 0);
        assert_eq!(dict.index_of_string("world"), 1);
        assert_eq!(dict.index_of_string("foo"), 2);
        assert_eq!(dict.index_of_string("bar"), NULL_VALUE_INDEX);

        assert_eq!(dict.get_string(0).unwrap(), "hello");
        assert_eq!(dict.get_string(1).unwrap(), "world");

        // Min/max should be lexicographic
        assert_eq!(
            dict.min_value(),
            Some(DictionaryValue::String("foo".to_string()))
        );
        assert_eq!(
            dict.max_value(),
            Some(DictionaryValue::String("world".to_string()))
        );
    }

    #[test]
    fn test_double_mutable_dictionary() {
        let mut dict = DoubleOnHeapMutableDictionary::new();

        dict.index(DictionaryValue::double(1.5)).unwrap();
        dict.index(DictionaryValue::double(2.5)).unwrap();
        dict.index(DictionaryValue::double(0.5)).unwrap();

        assert_eq!(dict.index_of_double(1.5), 0);
        assert_eq!(dict.index_of_double(2.5), 1);
        assert_eq!(dict.index_of_double(0.5), 2);

        assert!((dict.get_double(0).unwrap() - 1.5).abs() < f64::EPSILON);

        assert_eq!(dict.min_value(), Some(DictionaryValue::double(0.5)));
        assert_eq!(dict.max_value(), Some(DictionaryValue::double(2.5)));
    }

    #[test]
    fn test_mutable_dictionary_range_query() {
        let mut dict = IntOnHeapMutableDictionary::new();

        for i in 0..10 {
            dict.index(DictionaryValue::Int(i * 10)).unwrap();
        }

        // Range [20, 50] inclusive
        let in_range = dict.get_dict_ids_in_range(
            Some(&DictionaryValue::Int(20)),
            Some(&DictionaryValue::Int(50)),
            true,
            true,
        );

        assert_eq!(in_range.len(), 4);
        assert!(in_range.contains(&2)); // 20
        assert!(in_range.contains(&3)); // 30
        assert!(in_range.contains(&4)); // 40
        assert!(in_range.contains(&5)); // 50
    }

    #[test]
    fn test_mutable_dictionary_concurrent_access() {
        use std::sync::Arc;
        use std::thread;

        let dict = Arc::new(IntOnHeapMutableDictionary::new());
        let mut handles = vec![];

        // Multiple threads reading
        for _ in 0..4 {
            let dict_clone = Arc::clone(&dict);
            handles.push(thread::spawn(move || {
                for i in 0..1000 {
                    let _ = dict_clone.index_of_int(i);
                }
            }));
        }

        for handle in handles {
            handle.join().unwrap();
        }
    }

    #[test]
    fn test_large_dictionary() {
        let mut dict = IntOnHeapMutableDictionary::new();

        // Add more than one segment worth of entries
        for i in 0..20000 {
            dict.index(DictionaryValue::Int(i)).unwrap();
        }

        assert_eq!(dict.len(), 20000);

        // Verify values in different segments
        assert_eq!(dict.get_int(0).unwrap(), 0);
        assert_eq!(dict.get_int(8191).unwrap(), 8191);
        assert_eq!(dict.get_int(8192).unwrap(), 8192); // First entry in second segment
        assert_eq!(dict.get_int(19999).unwrap(), 19999);
    }

    #[test]
    fn test_factory() {
        let int_dict = MutableDictionaryFactory::create(DataType::Int);
        assert_eq!(int_dict.value_type(), DataType::Int);

        let string_dict = MutableDictionaryFactory::create(DataType::String);
        assert_eq!(string_dict.value_type(), DataType::String);
    }

    #[test]
    fn test_empty_dictionary() {
        let dict = IntOnHeapMutableDictionary::new();

        assert!(dict.is_empty());
        assert_eq!(dict.len(), 0);
        assert_eq!(dict.index_of_int(1), NULL_VALUE_INDEX);
        assert!(dict.get_int(0).is_err());
        assert_eq!(dict.min_value(), None);
        assert_eq!(dict.max_value(), None);
    }
}
