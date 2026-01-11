//! Result holders for aggregation functions
//!
//! These structures store intermediate and final aggregation results.

use super::{AggregationError, AggregationResult};
use serde::{Deserialize, Serialize};

/// Invalid group key marker
pub const INVALID_GROUP_KEY: i32 = -1;

/// Trait for aggregation result holders
pub trait ResultHolder: Send + Sync {
    /// Sets a double value
    fn set_double(&mut self, value: f64);

    /// Gets the double result
    fn get_double(&self) -> f64;

    /// Sets a long value
    fn set_long(&mut self, value: i64);

    /// Gets the long result
    fn get_long(&self) -> i64;

    /// Resets the holder to its initial state
    fn reset(&mut self);
}

/// Result holder for double values (used by SUM, MIN, MAX)
#[derive(Debug, Clone)]
pub struct DoubleResultHolder {
    value: f64,
    default_value: f64,
}

impl DoubleResultHolder {
    pub fn new(default_value: f64) -> Self {
        Self {
            value: default_value,
            default_value,
        }
    }

    pub fn sum() -> Self {
        Self::new(0.0)
    }

    pub fn min() -> Self {
        Self::new(f64::INFINITY)
    }

    pub fn max() -> Self {
        Self::new(f64::NEG_INFINITY)
    }
}

impl ResultHolder for DoubleResultHolder {
    fn set_double(&mut self, value: f64) {
        self.value = value;
    }

    fn get_double(&self) -> f64 {
        self.value
    }

    fn set_long(&mut self, value: i64) {
        self.value = value as f64;
    }

    fn get_long(&self) -> i64 {
        self.value as i64
    }

    fn reset(&mut self) {
        self.value = self.default_value;
    }
}

/// Result holder for long values (used by COUNT)
#[derive(Debug, Clone)]
pub struct LongResultHolder {
    value: i64,
    default_value: i64,
}

impl LongResultHolder {
    pub fn new(default_value: i64) -> Self {
        Self {
            value: default_value,
            default_value,
        }
    }

    pub fn count() -> Self {
        Self::new(0)
    }
}

impl ResultHolder for LongResultHolder {
    fn set_double(&mut self, value: f64) {
        self.value = value as i64;
    }

    fn get_double(&self) -> f64 {
        self.value as f64
    }

    fn set_long(&mut self, value: i64) {
        self.value = value;
    }

    fn get_long(&self) -> i64 {
        self.value
    }

    fn reset(&mut self) {
        self.value = self.default_value;
    }
}

/// Intermediate result for AVG aggregation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AvgPair {
    sum: f64,
    count: i64,
}

impl AvgPair {
    pub fn new() -> Self {
        Self { sum: 0.0, count: 0 }
    }

    pub fn with_values(sum: f64, count: i64) -> Self {
        Self { sum, count }
    }

    /// Applies a single value
    pub fn apply(&mut self, value: f64) {
        self.sum += value;
        self.count += 1;
    }

    /// Applies sum and count
    pub fn apply_with_count(&mut self, sum: f64, count: i64) {
        self.sum += sum;
        self.count += count;
    }

    /// Merges another AvgPair into this one
    pub fn merge(&mut self, other: &AvgPair) {
        self.sum += other.sum;
        self.count += other.count;
    }

    /// Computes the final average
    pub fn average(&self) -> Option<f64> {
        if self.count == 0 {
            None
        } else {
            Some(self.sum / self.count as f64)
        }
    }

    pub fn sum(&self) -> f64 {
        self.sum
    }

    pub fn count(&self) -> i64 {
        self.count
    }

    /// Serializes to bytes
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(16);
        bytes.extend_from_slice(&self.sum.to_be_bytes());
        bytes.extend_from_slice(&self.count.to_be_bytes());
        bytes
    }

    /// Deserializes from bytes
    pub fn from_bytes(bytes: &[u8]) -> AggregationResult<Self> {
        if bytes.len() < 16 {
            return Err(AggregationError::TypeMismatch {
                expected: "16 bytes".to_string(),
                actual: format!("{} bytes", bytes.len()),
            });
        }
        let sum = f64::from_be_bytes(bytes[0..8].try_into().unwrap());
        let count = i64::from_be_bytes(bytes[8..16].try_into().unwrap());
        Ok(Self { sum, count })
    }
}

impl Default for AvgPair {
    fn default() -> Self {
        Self::new()
    }
}

impl PartialEq for AvgPair {
    fn eq(&self, other: &Self) -> bool {
        (self.sum - other.sum).abs() < f64::EPSILON && self.count == other.count
    }
}

impl PartialOrd for AvgPair {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        match (self.average(), other.average()) {
            (Some(a), Some(b)) => a.partial_cmp(&b),
            (None, None) => Some(std::cmp::Ordering::Equal),
            (None, Some(_)) => Some(std::cmp::Ordering::Less),
            (Some(_), None) => Some(std::cmp::Ordering::Greater),
        }
    }
}

/// Result holder for AVG aggregation
#[derive(Debug, Clone)]
pub struct AvgResultHolder {
    pair: AvgPair,
}

impl AvgResultHolder {
    pub fn new() -> Self {
        Self {
            pair: AvgPair::new(),
        }
    }

    pub fn apply(&mut self, value: f64) {
        self.pair.apply(value);
    }

    pub fn apply_with_count(&mut self, sum: f64, count: i64) {
        self.pair.apply_with_count(sum, count);
    }

    pub fn merge(&mut self, other: &AvgPair) {
        self.pair.merge(other);
    }

    pub fn pair(&self) -> &AvgPair {
        &self.pair
    }

    pub fn average(&self) -> Option<f64> {
        self.pair.average()
    }
}

impl Default for AvgResultHolder {
    fn default() -> Self {
        Self::new()
    }
}

impl ResultHolder for AvgResultHolder {
    fn set_double(&mut self, value: f64) {
        self.pair.apply(value);
    }

    fn get_double(&self) -> f64 {
        self.pair.average().unwrap_or(0.0)
    }

    fn set_long(&mut self, value: i64) {
        self.pair.apply(value as f64);
    }

    fn get_long(&self) -> i64 {
        self.pair.average().map(|v| v as i64).unwrap_or(0)
    }

    fn reset(&mut self) {
        self.pair = AvgPair::new();
    }
}

// ========== GroupBy Result Holders ==========

/// Trait for group-by result holders
pub trait GroupByResultHolder: Send + Sync {
    /// Sets a double value for the given group key
    fn set_value_for_key_double(&mut self, group_key: i32, value: f64);

    /// Gets the double result for the given group key
    fn get_double_result(&self, group_key: i32) -> f64;

    /// Sets a long value for the given group key
    fn set_value_for_key_long(&mut self, group_key: i32, value: i64);

    /// Gets the long result for the given group key
    fn get_long_result(&self, group_key: i32) -> i64;

    /// Ensures the holder has capacity for the given number of groups
    fn ensure_capacity(&mut self, capacity: usize);

    /// Returns the current capacity
    fn capacity(&self) -> usize;
}

/// Group-by result holder for double values
pub struct DoubleGroupByResultHolder {
    results: Vec<f64>,
    default_value: f64,
    max_capacity: usize,
}

impl DoubleGroupByResultHolder {
    pub fn new(initial_capacity: usize, max_capacity: usize, default_value: f64) -> Self {
        let mut results = vec![default_value; initial_capacity];
        if default_value != 0.0 {
            results.fill(default_value);
        }
        Self {
            results,
            default_value,
            max_capacity,
        }
    }

    pub fn for_sum(initial_capacity: usize, max_capacity: usize) -> Self {
        Self::new(initial_capacity, max_capacity, 0.0)
    }

    pub fn for_min(initial_capacity: usize, max_capacity: usize) -> Self {
        Self::new(initial_capacity, max_capacity, f64::INFINITY)
    }

    pub fn for_max(initial_capacity: usize, max_capacity: usize) -> Self {
        Self::new(initial_capacity, max_capacity, f64::NEG_INFINITY)
    }

    /// Adds a value to the current value for the group
    pub fn add_value_for_key(&mut self, group_key: i32, value: f64) {
        if group_key != INVALID_GROUP_KEY && (group_key as usize) < self.results.len() {
            self.results[group_key as usize] += value;
        }
    }

    /// Updates with minimum value
    pub fn min_value_for_key(&mut self, group_key: i32, value: f64) {
        if group_key != INVALID_GROUP_KEY && (group_key as usize) < self.results.len() {
            let current = self.results[group_key as usize];
            if value < current {
                self.results[group_key as usize] = value;
            }
        }
    }

    /// Updates with maximum value
    pub fn max_value_for_key(&mut self, group_key: i32, value: f64) {
        if group_key != INVALID_GROUP_KEY && (group_key as usize) < self.results.len() {
            let current = self.results[group_key as usize];
            if value > current {
                self.results[group_key as usize] = value;
            }
        }
    }
}

impl GroupByResultHolder for DoubleGroupByResultHolder {
    fn set_value_for_key_double(&mut self, group_key: i32, value: f64) {
        if group_key != INVALID_GROUP_KEY && (group_key as usize) < self.results.len() {
            self.results[group_key as usize] = value;
        }
    }

    fn get_double_result(&self, group_key: i32) -> f64 {
        if group_key == INVALID_GROUP_KEY {
            return self.default_value;
        }
        self.results
            .get(group_key as usize)
            .copied()
            .unwrap_or(self.default_value)
    }

    fn set_value_for_key_long(&mut self, group_key: i32, value: i64) {
        self.set_value_for_key_double(group_key, value as f64);
    }

    fn get_long_result(&self, group_key: i32) -> i64 {
        self.get_double_result(group_key) as i64
    }

    fn ensure_capacity(&mut self, capacity: usize) {
        if capacity > self.results.len() {
            let new_capacity = (self.results.len() * 2).max(capacity).min(self.max_capacity);
            let old_len = self.results.len();
            self.results.resize(new_capacity, self.default_value);
            if self.default_value != 0.0 {
                for i in old_len..new_capacity {
                    self.results[i] = self.default_value;
                }
            }
        }
    }

    fn capacity(&self) -> usize {
        self.results.len()
    }
}

/// Group-by result holder for long values (COUNT)
pub struct LongGroupByResultHolder {
    results: Vec<i64>,
    default_value: i64,
    max_capacity: usize,
}

impl LongGroupByResultHolder {
    pub fn new(initial_capacity: usize, max_capacity: usize, default_value: i64) -> Self {
        Self {
            results: vec![default_value; initial_capacity],
            default_value,
            max_capacity,
        }
    }

    pub fn for_count(initial_capacity: usize, max_capacity: usize) -> Self {
        Self::new(initial_capacity, max_capacity, 0)
    }

    /// Increments the count for the group
    pub fn increment(&mut self, group_key: i32) {
        if group_key != INVALID_GROUP_KEY && (group_key as usize) < self.results.len() {
            self.results[group_key as usize] += 1;
        }
    }

    /// Adds a value to the count for the group
    pub fn add(&mut self, group_key: i32, value: i64) {
        if group_key != INVALID_GROUP_KEY && (group_key as usize) < self.results.len() {
            self.results[group_key as usize] += value;
        }
    }
}

impl GroupByResultHolder for LongGroupByResultHolder {
    fn set_value_for_key_double(&mut self, group_key: i32, value: f64) {
        self.set_value_for_key_long(group_key, value as i64);
    }

    fn get_double_result(&self, group_key: i32) -> f64 {
        self.get_long_result(group_key) as f64
    }

    fn set_value_for_key_long(&mut self, group_key: i32, value: i64) {
        if group_key != INVALID_GROUP_KEY && (group_key as usize) < self.results.len() {
            self.results[group_key as usize] = value;
        }
    }

    fn get_long_result(&self, group_key: i32) -> i64 {
        if group_key == INVALID_GROUP_KEY {
            return self.default_value;
        }
        self.results
            .get(group_key as usize)
            .copied()
            .unwrap_or(self.default_value)
    }

    fn ensure_capacity(&mut self, capacity: usize) {
        if capacity > self.results.len() {
            let new_capacity = (self.results.len() * 2).max(capacity).min(self.max_capacity);
            self.results.resize(new_capacity, self.default_value);
        }
    }

    fn capacity(&self) -> usize {
        self.results.len()
    }
}

/// Group-by result holder for AVG (stores AvgPair per group)
pub struct AvgGroupByResultHolder {
    results: Vec<AvgPair>,
    max_capacity: usize,
}

impl AvgGroupByResultHolder {
    pub fn new(initial_capacity: usize, max_capacity: usize) -> Self {
        Self {
            results: vec![AvgPair::new(); initial_capacity],
            max_capacity,
        }
    }

    /// Applies a value to the group
    pub fn apply(&mut self, group_key: i32, value: f64) {
        if group_key != INVALID_GROUP_KEY && (group_key as usize) < self.results.len() {
            self.results[group_key as usize].apply(value);
        }
    }

    /// Gets the AvgPair for a group
    pub fn get_pair(&self, group_key: i32) -> Option<&AvgPair> {
        if group_key == INVALID_GROUP_KEY {
            return None;
        }
        self.results.get(group_key as usize)
    }

    /// Merges an AvgPair into a group
    pub fn merge(&mut self, group_key: i32, pair: &AvgPair) {
        if group_key != INVALID_GROUP_KEY && (group_key as usize) < self.results.len() {
            self.results[group_key as usize].merge(pair);
        }
    }
}

impl GroupByResultHolder for AvgGroupByResultHolder {
    fn set_value_for_key_double(&mut self, group_key: i32, value: f64) {
        self.apply(group_key, value);
    }

    fn get_double_result(&self, group_key: i32) -> f64 {
        self.get_pair(group_key)
            .and_then(|p| p.average())
            .unwrap_or(0.0)
    }

    fn set_value_for_key_long(&mut self, group_key: i32, value: i64) {
        self.apply(group_key, value as f64);
    }

    fn get_long_result(&self, group_key: i32) -> i64 {
        self.get_double_result(group_key) as i64
    }

    fn ensure_capacity(&mut self, capacity: usize) {
        if capacity > self.results.len() {
            let new_capacity = (self.results.len() * 2).max(capacity).min(self.max_capacity);
            self.results.resize(new_capacity, AvgPair::new());
        }
    }

    fn capacity(&self) -> usize {
        self.results.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_double_result_holder() {
        let mut holder = DoubleResultHolder::sum();
        assert_eq!(holder.get_double(), 0.0);

        holder.set_double(42.5);
        assert_eq!(holder.get_double(), 42.5);

        holder.reset();
        assert_eq!(holder.get_double(), 0.0);
    }

    #[test]
    fn test_min_max_result_holders() {
        let holder = DoubleResultHolder::min();
        assert_eq!(holder.get_double(), f64::INFINITY);

        let holder = DoubleResultHolder::max();
        assert_eq!(holder.get_double(), f64::NEG_INFINITY);
    }

    #[test]
    fn test_avg_pair() {
        let mut pair = AvgPair::new();

        pair.apply(10.0);
        pair.apply(20.0);
        pair.apply(30.0);

        assert_eq!(pair.sum(), 60.0);
        assert_eq!(pair.count(), 3);
        assert!((pair.average().unwrap() - 20.0).abs() < f64::EPSILON);

        // Test merge
        let mut pair2 = AvgPair::new();
        pair2.apply(40.0);
        pair2.apply(50.0);

        pair.merge(&pair2);
        assert_eq!(pair.sum(), 150.0);
        assert_eq!(pair.count(), 5);
        assert!((pair.average().unwrap() - 30.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_avg_pair_serialization() {
        let pair = AvgPair::with_values(100.5, 10);
        let bytes = pair.to_bytes();
        let restored = AvgPair::from_bytes(&bytes).unwrap();

        assert!((pair.sum() - restored.sum()).abs() < f64::EPSILON);
        assert_eq!(pair.count(), restored.count());
    }

    #[test]
    fn test_double_group_by_holder() {
        let mut holder = DoubleGroupByResultHolder::for_sum(10, 1000);

        holder.add_value_for_key(0, 10.0);
        holder.add_value_for_key(0, 20.0);
        holder.add_value_for_key(1, 30.0);

        assert_eq!(holder.get_double_result(0), 30.0);
        assert_eq!(holder.get_double_result(1), 30.0);
        assert_eq!(holder.get_double_result(2), 0.0); // Default
    }

    #[test]
    fn test_group_by_holder_min_max() {
        let mut min_holder = DoubleGroupByResultHolder::for_min(10, 1000);
        min_holder.min_value_for_key(0, 50.0);
        min_holder.min_value_for_key(0, 30.0);
        min_holder.min_value_for_key(0, 40.0);
        assert_eq!(min_holder.get_double_result(0), 30.0);

        let mut max_holder = DoubleGroupByResultHolder::for_max(10, 1000);
        max_holder.max_value_for_key(0, 50.0);
        max_holder.max_value_for_key(0, 30.0);
        max_holder.max_value_for_key(0, 40.0);
        assert_eq!(max_holder.get_double_result(0), 50.0);
    }

    #[test]
    fn test_long_group_by_holder() {
        let mut holder = LongGroupByResultHolder::for_count(10, 1000);

        holder.increment(0);
        holder.increment(0);
        holder.increment(1);

        assert_eq!(holder.get_long_result(0), 2);
        assert_eq!(holder.get_long_result(1), 1);
    }

    #[test]
    fn test_avg_group_by_holder() {
        let mut holder = AvgGroupByResultHolder::new(10, 1000);

        holder.apply(0, 10.0);
        holder.apply(0, 20.0);
        holder.apply(0, 30.0);

        let pair = holder.get_pair(0).unwrap();
        assert_eq!(pair.count(), 3);
        assert!((pair.average().unwrap() - 20.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_group_by_holder_capacity() {
        let mut holder = DoubleGroupByResultHolder::for_sum(10, 1000);
        assert_eq!(holder.capacity(), 10);

        holder.ensure_capacity(50);
        assert!(holder.capacity() >= 50);

        // Test that defaults are preserved after resize
        assert_eq!(holder.get_double_result(0), 0.0);
    }

    #[test]
    fn test_invalid_group_key() {
        let holder = DoubleGroupByResultHolder::for_sum(10, 1000);
        assert_eq!(holder.get_double_result(INVALID_GROUP_KEY), 0.0);

        let mut holder = DoubleGroupByResultHolder::for_sum(10, 1000);
        holder.set_value_for_key_double(INVALID_GROUP_KEY, 100.0);
        // Should not crash, value should not be set
    }
}
