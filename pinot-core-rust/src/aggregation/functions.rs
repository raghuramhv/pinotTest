//! Aggregation function implementations
//!
//! Each aggregation function implements the core aggregation logic
//! for both single-row and group-by operations.

use super::result_holders::*;
use super::{AggregationResult, AggregationType};
use roaring::RoaringBitmap;

/// Trait for aggregation functions
pub trait AggregationFunction: Send + Sync {
    /// Returns the aggregation type
    fn aggregation_type(&self) -> AggregationType;

    /// Creates a new result holder for non-grouped aggregation
    fn create_result_holder(&self) -> Box<dyn ResultHolder>;

    /// Creates a new result holder for group-by aggregation
    fn create_group_by_result_holder(
        &self,
        initial_capacity: usize,
        max_capacity: usize,
    ) -> Box<dyn GroupByResultHolder>;

    /// Aggregates double values into a result holder
    fn aggregate_double(
        &self,
        values: &[f64],
        result_holder: &mut dyn ResultHolder,
        null_bitmap: Option<&RoaringBitmap>,
    );

    /// Aggregates long values into a result holder
    fn aggregate_long(
        &self,
        values: &[i64],
        result_holder: &mut dyn ResultHolder,
        null_bitmap: Option<&RoaringBitmap>,
    );

    /// Aggregates double values with group keys
    fn aggregate_double_group_by(
        &self,
        values: &[f64],
        group_keys: &[i32],
        result_holder: &mut dyn GroupByResultHolder,
        null_bitmap: Option<&RoaringBitmap>,
    );

    /// Aggregates long values with group keys
    fn aggregate_long_group_by(
        &self,
        values: &[i64],
        group_keys: &[i32],
        result_holder: &mut dyn GroupByResultHolder,
        null_bitmap: Option<&RoaringBitmap>,
    );

    /// Extracts the final result from a result holder
    fn extract_result(&self, result_holder: &dyn ResultHolder) -> AggregationResult<f64>;

    /// Merges two intermediate results
    fn merge(&self, result1: f64, result2: f64) -> f64;
}

/// SUM aggregation function
#[derive(Debug, Default, Clone)]
pub struct SumAggregationFunction;

impl SumAggregationFunction {
    pub fn new() -> Self {
        Self
    }
}

impl AggregationFunction for SumAggregationFunction {
    fn aggregation_type(&self) -> AggregationType {
        AggregationType::Sum
    }

    fn create_result_holder(&self) -> Box<dyn ResultHolder> {
        Box::new(DoubleResultHolder::sum())
    }

    fn create_group_by_result_holder(
        &self,
        initial_capacity: usize,
        max_capacity: usize,
    ) -> Box<dyn GroupByResultHolder> {
        Box::new(DoubleGroupByResultHolder::for_sum(
            initial_capacity,
            max_capacity,
        ))
    }

    fn aggregate_double(
        &self,
        values: &[f64],
        result_holder: &mut dyn ResultHolder,
        null_bitmap: Option<&RoaringBitmap>,
    ) {
        let sum: f64 = match null_bitmap {
            Some(nulls) => values
                .iter()
                .enumerate()
                .filter(|(i, _)| !nulls.contains(*i as u32))
                .map(|(_, v)| v)
                .sum(),
            None => values.iter().sum(),
        };
        result_holder.set_double(result_holder.get_double() + sum);
    }

    fn aggregate_long(
        &self,
        values: &[i64],
        result_holder: &mut dyn ResultHolder,
        null_bitmap: Option<&RoaringBitmap>,
    ) {
        let sum: i64 = match null_bitmap {
            Some(nulls) => values
                .iter()
                .enumerate()
                .filter(|(i, _)| !nulls.contains(*i as u32))
                .map(|(_, v)| v)
                .sum(),
            None => values.iter().sum(),
        };
        result_holder.set_double(result_holder.get_double() + sum as f64);
    }

    fn aggregate_double_group_by(
        &self,
        values: &[f64],
        group_keys: &[i32],
        result_holder: &mut dyn GroupByResultHolder,
        null_bitmap: Option<&RoaringBitmap>,
    ) {
        // Downcast to get direct access for better performance
        let holder = unsafe {
            &mut *(result_holder as *mut dyn GroupByResultHolder as *mut DoubleGroupByResultHolder)
        };

        match null_bitmap {
            Some(nulls) => {
                for (i, (&value, &group_key)) in values.iter().zip(group_keys.iter()).enumerate() {
                    if !nulls.contains(i as u32) {
                        holder.add_value_for_key(group_key, value);
                    }
                }
            }
            None => {
                for (&value, &group_key) in values.iter().zip(group_keys.iter()) {
                    holder.add_value_for_key(group_key, value);
                }
            }
        }
    }

    fn aggregate_long_group_by(
        &self,
        values: &[i64],
        group_keys: &[i32],
        result_holder: &mut dyn GroupByResultHolder,
        null_bitmap: Option<&RoaringBitmap>,
    ) {
        let holder = unsafe {
            &mut *(result_holder as *mut dyn GroupByResultHolder as *mut DoubleGroupByResultHolder)
        };

        match null_bitmap {
            Some(nulls) => {
                for (i, (&value, &group_key)) in values.iter().zip(group_keys.iter()).enumerate() {
                    if !nulls.contains(i as u32) {
                        holder.add_value_for_key(group_key, value as f64);
                    }
                }
            }
            None => {
                for (&value, &group_key) in values.iter().zip(group_keys.iter()) {
                    holder.add_value_for_key(group_key, value as f64);
                }
            }
        }
    }

    fn extract_result(&self, result_holder: &dyn ResultHolder) -> AggregationResult<f64> {
        Ok(result_holder.get_double())
    }

    fn merge(&self, result1: f64, result2: f64) -> f64 {
        result1 + result2
    }
}

/// COUNT aggregation function
#[derive(Debug, Default, Clone)]
pub struct CountAggregationFunction;

impl CountAggregationFunction {
    pub fn new() -> Self {
        Self
    }
}

impl AggregationFunction for CountAggregationFunction {
    fn aggregation_type(&self) -> AggregationType {
        AggregationType::Count
    }

    fn create_result_holder(&self) -> Box<dyn ResultHolder> {
        Box::new(LongResultHolder::count())
    }

    fn create_group_by_result_holder(
        &self,
        initial_capacity: usize,
        max_capacity: usize,
    ) -> Box<dyn GroupByResultHolder> {
        Box::new(LongGroupByResultHolder::for_count(
            initial_capacity,
            max_capacity,
        ))
    }

    fn aggregate_double(
        &self,
        values: &[f64],
        result_holder: &mut dyn ResultHolder,
        null_bitmap: Option<&RoaringBitmap>,
    ) {
        let count = match null_bitmap {
            Some(nulls) => values.len() - nulls.len() as usize,
            None => values.len(),
        };
        result_holder.set_long(result_holder.get_long() + count as i64);
    }

    fn aggregate_long(
        &self,
        values: &[i64],
        result_holder: &mut dyn ResultHolder,
        null_bitmap: Option<&RoaringBitmap>,
    ) {
        let count = match null_bitmap {
            Some(nulls) => values.len() - nulls.len() as usize,
            None => values.len(),
        };
        result_holder.set_long(result_holder.get_long() + count as i64);
    }

    fn aggregate_double_group_by(
        &self,
        _values: &[f64],
        group_keys: &[i32],
        result_holder: &mut dyn GroupByResultHolder,
        null_bitmap: Option<&RoaringBitmap>,
    ) {
        let holder = unsafe {
            &mut *(result_holder as *mut dyn GroupByResultHolder as *mut LongGroupByResultHolder)
        };

        match null_bitmap {
            Some(nulls) => {
                for (i, &group_key) in group_keys.iter().enumerate() {
                    if !nulls.contains(i as u32) {
                        holder.increment(group_key);
                    }
                }
            }
            None => {
                for &group_key in group_keys.iter() {
                    holder.increment(group_key);
                }
            }
        }
    }

    fn aggregate_long_group_by(
        &self,
        _values: &[i64],
        group_keys: &[i32],
        result_holder: &mut dyn GroupByResultHolder,
        null_bitmap: Option<&RoaringBitmap>,
    ) {
        let holder = unsafe {
            &mut *(result_holder as *mut dyn GroupByResultHolder as *mut LongGroupByResultHolder)
        };

        match null_bitmap {
            Some(nulls) => {
                for (i, &group_key) in group_keys.iter().enumerate() {
                    if !nulls.contains(i as u32) {
                        holder.increment(group_key);
                    }
                }
            }
            None => {
                for &group_key in group_keys.iter() {
                    holder.increment(group_key);
                }
            }
        }
    }

    fn extract_result(&self, result_holder: &dyn ResultHolder) -> AggregationResult<f64> {
        Ok(result_holder.get_long() as f64)
    }

    fn merge(&self, result1: f64, result2: f64) -> f64 {
        result1 + result2
    }
}

/// MIN aggregation function
#[derive(Debug, Default, Clone)]
pub struct MinAggregationFunction;

impl MinAggregationFunction {
    pub fn new() -> Self {
        Self
    }
}

impl AggregationFunction for MinAggregationFunction {
    fn aggregation_type(&self) -> AggregationType {
        AggregationType::Min
    }

    fn create_result_holder(&self) -> Box<dyn ResultHolder> {
        Box::new(DoubleResultHolder::min())
    }

    fn create_group_by_result_holder(
        &self,
        initial_capacity: usize,
        max_capacity: usize,
    ) -> Box<dyn GroupByResultHolder> {
        Box::new(DoubleGroupByResultHolder::for_min(
            initial_capacity,
            max_capacity,
        ))
    }

    fn aggregate_double(
        &self,
        values: &[f64],
        result_holder: &mut dyn ResultHolder,
        null_bitmap: Option<&RoaringBitmap>,
    ) {
        let min = match null_bitmap {
            Some(nulls) => values
                .iter()
                .enumerate()
                .filter(|(i, _)| !nulls.contains(*i as u32))
                .map(|(_, v)| *v)
                .fold(f64::INFINITY, f64::min),
            None => values.iter().copied().fold(f64::INFINITY, f64::min),
        };
        let current = result_holder.get_double();
        result_holder.set_double(current.min(min));
    }

    fn aggregate_long(
        &self,
        values: &[i64],
        result_holder: &mut dyn ResultHolder,
        null_bitmap: Option<&RoaringBitmap>,
    ) {
        let min = match null_bitmap {
            Some(nulls) => values
                .iter()
                .enumerate()
                .filter(|(i, _)| !nulls.contains(*i as u32))
                .map(|(_, v)| *v as f64)
                .fold(f64::INFINITY, f64::min),
            None => values.iter().map(|v| *v as f64).fold(f64::INFINITY, f64::min),
        };
        let current = result_holder.get_double();
        result_holder.set_double(current.min(min));
    }

    fn aggregate_double_group_by(
        &self,
        values: &[f64],
        group_keys: &[i32],
        result_holder: &mut dyn GroupByResultHolder,
        null_bitmap: Option<&RoaringBitmap>,
    ) {
        let holder = unsafe {
            &mut *(result_holder as *mut dyn GroupByResultHolder as *mut DoubleGroupByResultHolder)
        };

        match null_bitmap {
            Some(nulls) => {
                for (i, (&value, &group_key)) in values.iter().zip(group_keys.iter()).enumerate() {
                    if !nulls.contains(i as u32) {
                        holder.min_value_for_key(group_key, value);
                    }
                }
            }
            None => {
                for (&value, &group_key) in values.iter().zip(group_keys.iter()) {
                    holder.min_value_for_key(group_key, value);
                }
            }
        }
    }

    fn aggregate_long_group_by(
        &self,
        values: &[i64],
        group_keys: &[i32],
        result_holder: &mut dyn GroupByResultHolder,
        null_bitmap: Option<&RoaringBitmap>,
    ) {
        let holder = unsafe {
            &mut *(result_holder as *mut dyn GroupByResultHolder as *mut DoubleGroupByResultHolder)
        };

        match null_bitmap {
            Some(nulls) => {
                for (i, (&value, &group_key)) in values.iter().zip(group_keys.iter()).enumerate() {
                    if !nulls.contains(i as u32) {
                        holder.min_value_for_key(group_key, value as f64);
                    }
                }
            }
            None => {
                for (&value, &group_key) in values.iter().zip(group_keys.iter()) {
                    holder.min_value_for_key(group_key, value as f64);
                }
            }
        }
    }

    fn extract_result(&self, result_holder: &dyn ResultHolder) -> AggregationResult<f64> {
        Ok(result_holder.get_double())
    }

    fn merge(&self, result1: f64, result2: f64) -> f64 {
        result1.min(result2)
    }
}

/// MAX aggregation function
#[derive(Debug, Default, Clone)]
pub struct MaxAggregationFunction;

impl MaxAggregationFunction {
    pub fn new() -> Self {
        Self
    }
}

impl AggregationFunction for MaxAggregationFunction {
    fn aggregation_type(&self) -> AggregationType {
        AggregationType::Max
    }

    fn create_result_holder(&self) -> Box<dyn ResultHolder> {
        Box::new(DoubleResultHolder::max())
    }

    fn create_group_by_result_holder(
        &self,
        initial_capacity: usize,
        max_capacity: usize,
    ) -> Box<dyn GroupByResultHolder> {
        Box::new(DoubleGroupByResultHolder::for_max(
            initial_capacity,
            max_capacity,
        ))
    }

    fn aggregate_double(
        &self,
        values: &[f64],
        result_holder: &mut dyn ResultHolder,
        null_bitmap: Option<&RoaringBitmap>,
    ) {
        let max = match null_bitmap {
            Some(nulls) => values
                .iter()
                .enumerate()
                .filter(|(i, _)| !nulls.contains(*i as u32))
                .map(|(_, v)| *v)
                .fold(f64::NEG_INFINITY, f64::max),
            None => values.iter().copied().fold(f64::NEG_INFINITY, f64::max),
        };
        let current = result_holder.get_double();
        result_holder.set_double(current.max(max));
    }

    fn aggregate_long(
        &self,
        values: &[i64],
        result_holder: &mut dyn ResultHolder,
        null_bitmap: Option<&RoaringBitmap>,
    ) {
        let max = match null_bitmap {
            Some(nulls) => values
                .iter()
                .enumerate()
                .filter(|(i, _)| !nulls.contains(*i as u32))
                .map(|(_, v)| *v as f64)
                .fold(f64::NEG_INFINITY, f64::max),
            None => values
                .iter()
                .map(|v| *v as f64)
                .fold(f64::NEG_INFINITY, f64::max),
        };
        let current = result_holder.get_double();
        result_holder.set_double(current.max(max));
    }

    fn aggregate_double_group_by(
        &self,
        values: &[f64],
        group_keys: &[i32],
        result_holder: &mut dyn GroupByResultHolder,
        null_bitmap: Option<&RoaringBitmap>,
    ) {
        let holder = unsafe {
            &mut *(result_holder as *mut dyn GroupByResultHolder as *mut DoubleGroupByResultHolder)
        };

        match null_bitmap {
            Some(nulls) => {
                for (i, (&value, &group_key)) in values.iter().zip(group_keys.iter()).enumerate() {
                    if !nulls.contains(i as u32) {
                        holder.max_value_for_key(group_key, value);
                    }
                }
            }
            None => {
                for (&value, &group_key) in values.iter().zip(group_keys.iter()) {
                    holder.max_value_for_key(group_key, value);
                }
            }
        }
    }

    fn aggregate_long_group_by(
        &self,
        values: &[i64],
        group_keys: &[i32],
        result_holder: &mut dyn GroupByResultHolder,
        null_bitmap: Option<&RoaringBitmap>,
    ) {
        let holder = unsafe {
            &mut *(result_holder as *mut dyn GroupByResultHolder as *mut DoubleGroupByResultHolder)
        };

        match null_bitmap {
            Some(nulls) => {
                for (i, (&value, &group_key)) in values.iter().zip(group_keys.iter()).enumerate() {
                    if !nulls.contains(i as u32) {
                        holder.max_value_for_key(group_key, value as f64);
                    }
                }
            }
            None => {
                for (&value, &group_key) in values.iter().zip(group_keys.iter()) {
                    holder.max_value_for_key(group_key, value as f64);
                }
            }
        }
    }

    fn extract_result(&self, result_holder: &dyn ResultHolder) -> AggregationResult<f64> {
        Ok(result_holder.get_double())
    }

    fn merge(&self, result1: f64, result2: f64) -> f64 {
        result1.max(result2)
    }
}

/// AVG aggregation function
#[derive(Debug, Default, Clone)]
pub struct AvgAggregationFunction;

impl AvgAggregationFunction {
    pub fn new() -> Self {
        Self
    }
}

impl AggregationFunction for AvgAggregationFunction {
    fn aggregation_type(&self) -> AggregationType {
        AggregationType::Avg
    }

    fn create_result_holder(&self) -> Box<dyn ResultHolder> {
        Box::new(AvgResultHolder::new())
    }

    fn create_group_by_result_holder(
        &self,
        initial_capacity: usize,
        max_capacity: usize,
    ) -> Box<dyn GroupByResultHolder> {
        Box::new(AvgGroupByResultHolder::new(initial_capacity, max_capacity))
    }

    fn aggregate_double(
        &self,
        values: &[f64],
        result_holder: &mut dyn ResultHolder,
        null_bitmap: Option<&RoaringBitmap>,
    ) {
        // Downcast to AvgResultHolder
        let holder =
            unsafe { &mut *(result_holder as *mut dyn ResultHolder as *mut AvgResultHolder) };

        match null_bitmap {
            Some(nulls) => {
                for (i, &value) in values.iter().enumerate() {
                    if !nulls.contains(i as u32) {
                        holder.apply(value);
                    }
                }
            }
            None => {
                for &value in values.iter() {
                    holder.apply(value);
                }
            }
        }
    }

    fn aggregate_long(
        &self,
        values: &[i64],
        result_holder: &mut dyn ResultHolder,
        null_bitmap: Option<&RoaringBitmap>,
    ) {
        let holder =
            unsafe { &mut *(result_holder as *mut dyn ResultHolder as *mut AvgResultHolder) };

        match null_bitmap {
            Some(nulls) => {
                for (i, &value) in values.iter().enumerate() {
                    if !nulls.contains(i as u32) {
                        holder.apply(value as f64);
                    }
                }
            }
            None => {
                for &value in values.iter() {
                    holder.apply(value as f64);
                }
            }
        }
    }

    fn aggregate_double_group_by(
        &self,
        values: &[f64],
        group_keys: &[i32],
        result_holder: &mut dyn GroupByResultHolder,
        null_bitmap: Option<&RoaringBitmap>,
    ) {
        let holder = unsafe {
            &mut *(result_holder as *mut dyn GroupByResultHolder as *mut AvgGroupByResultHolder)
        };

        match null_bitmap {
            Some(nulls) => {
                for (i, (&value, &group_key)) in values.iter().zip(group_keys.iter()).enumerate() {
                    if !nulls.contains(i as u32) {
                        holder.apply(group_key, value);
                    }
                }
            }
            None => {
                for (&value, &group_key) in values.iter().zip(group_keys.iter()) {
                    holder.apply(group_key, value);
                }
            }
        }
    }

    fn aggregate_long_group_by(
        &self,
        values: &[i64],
        group_keys: &[i32],
        result_holder: &mut dyn GroupByResultHolder,
        null_bitmap: Option<&RoaringBitmap>,
    ) {
        let holder = unsafe {
            &mut *(result_holder as *mut dyn GroupByResultHolder as *mut AvgGroupByResultHolder)
        };

        match null_bitmap {
            Some(nulls) => {
                for (i, (&value, &group_key)) in values.iter().zip(group_keys.iter()).enumerate() {
                    if !nulls.contains(i as u32) {
                        holder.apply(group_key, value as f64);
                    }
                }
            }
            None => {
                for (&value, &group_key) in values.iter().zip(group_keys.iter()) {
                    holder.apply(group_key, value as f64);
                }
            }
        }
    }

    fn extract_result(&self, result_holder: &dyn ResultHolder) -> AggregationResult<f64> {
        Ok(result_holder.get_double())
    }

    fn merge(&self, _result1: f64, _result2: f64) -> f64 {
        // For AVG, merging requires AvgPair, this is a simplified version
        // In practice, you'd merge AvgPairs and then compute the average
        panic!("Use merge_pairs for AVG aggregation")
    }
}

impl AvgAggregationFunction {
    /// Merges two AvgPairs
    pub fn merge_pairs(&self, pair1: &AvgPair, pair2: &AvgPair) -> AvgPair {
        let mut result = pair1.clone();
        result.merge(pair2);
        result
    }
}

/// Factory for creating aggregation functions
pub struct AggregationFunctionFactory;

impl AggregationFunctionFactory {
    pub fn create(aggregation_type: AggregationType) -> Box<dyn AggregationFunction> {
        match aggregation_type {
            AggregationType::Sum => Box::new(SumAggregationFunction::new()),
            AggregationType::Count => Box::new(CountAggregationFunction::new()),
            AggregationType::Min => Box::new(MinAggregationFunction::new()),
            AggregationType::Max => Box::new(MaxAggregationFunction::new()),
            AggregationType::Avg => Box::new(AvgAggregationFunction::new()),
            AggregationType::DistinctCount => {
                panic!("DistinctCount not yet implemented")
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sum_aggregation() {
        let func = SumAggregationFunction::new();
        let mut holder = func.create_result_holder();

        let values = vec![1.0, 2.0, 3.0, 4.0, 5.0];
        func.aggregate_double(&values, holder.as_mut(), None);

        assert!((holder.get_double() - 15.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_sum_with_nulls() {
        let func = SumAggregationFunction::new();
        let mut holder = func.create_result_holder();

        let values = vec![1.0, 2.0, 3.0, 4.0, 5.0];
        let mut nulls = RoaringBitmap::new();
        nulls.insert(1); // Mark index 1 as null (value 2.0)
        nulls.insert(3); // Mark index 3 as null (value 4.0)

        func.aggregate_double(&values, holder.as_mut(), Some(&nulls));

        // Sum should be 1.0 + 3.0 + 5.0 = 9.0
        assert!((holder.get_double() - 9.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_count_aggregation() {
        let func = CountAggregationFunction::new();
        let mut holder = func.create_result_holder();

        let values = vec![1.0, 2.0, 3.0, 4.0, 5.0];
        func.aggregate_double(&values, holder.as_mut(), None);

        assert_eq!(holder.get_long(), 5);
    }

    #[test]
    fn test_count_with_nulls() {
        let func = CountAggregationFunction::new();
        let mut holder = func.create_result_holder();

        let values = vec![1.0, 2.0, 3.0, 4.0, 5.0];
        let mut nulls = RoaringBitmap::new();
        nulls.insert(1);
        nulls.insert(3);

        func.aggregate_double(&values, holder.as_mut(), Some(&nulls));

        assert_eq!(holder.get_long(), 3);
    }

    #[test]
    fn test_min_aggregation() {
        let func = MinAggregationFunction::new();
        let mut holder = func.create_result_holder();

        let values = vec![5.0, 2.0, 8.0, 1.0, 9.0];
        func.aggregate_double(&values, holder.as_mut(), None);

        assert!((holder.get_double() - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_max_aggregation() {
        let func = MaxAggregationFunction::new();
        let mut holder = func.create_result_holder();

        let values = vec![5.0, 2.0, 8.0, 1.0, 9.0];
        func.aggregate_double(&values, holder.as_mut(), None);

        assert!((holder.get_double() - 9.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_avg_aggregation() {
        let func = AvgAggregationFunction::new();
        let mut holder = func.create_result_holder();

        let values = vec![10.0, 20.0, 30.0];
        func.aggregate_double(&values, holder.as_mut(), None);

        assert!((holder.get_double() - 20.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_sum_group_by() {
        let func = SumAggregationFunction::new();
        let mut holder = func.create_group_by_result_holder(10, 100);

        let values = vec![1.0, 2.0, 3.0, 4.0, 5.0, 6.0];
        let group_keys = vec![0, 0, 1, 1, 2, 2];

        func.aggregate_double_group_by(&values, &group_keys, holder.as_mut(), None);

        assert!((holder.get_double_result(0) - 3.0).abs() < f64::EPSILON); // 1 + 2
        assert!((holder.get_double_result(1) - 7.0).abs() < f64::EPSILON); // 3 + 4
        assert!((holder.get_double_result(2) - 11.0).abs() < f64::EPSILON); // 5 + 6
    }

    #[test]
    fn test_count_group_by() {
        let func = CountAggregationFunction::new();
        let mut holder = func.create_group_by_result_holder(10, 100);

        let values = vec![1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0];
        let group_keys = vec![0, 0, 0, 1, 1, 2, 2];

        func.aggregate_double_group_by(&values, &group_keys, holder.as_mut(), None);

        assert_eq!(holder.get_long_result(0), 3);
        assert_eq!(holder.get_long_result(1), 2);
        assert_eq!(holder.get_long_result(2), 2);
    }

    #[test]
    fn test_min_group_by() {
        let func = MinAggregationFunction::new();
        let mut holder = func.create_group_by_result_holder(10, 100);

        let values = vec![5.0, 2.0, 8.0, 1.0, 3.0, 4.0];
        let group_keys = vec![0, 0, 0, 1, 1, 1];

        func.aggregate_double_group_by(&values, &group_keys, holder.as_mut(), None);

        assert!((holder.get_double_result(0) - 2.0).abs() < f64::EPSILON);
        assert!((holder.get_double_result(1) - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_max_group_by() {
        let func = MaxAggregationFunction::new();
        let mut holder = func.create_group_by_result_holder(10, 100);

        let values = vec![5.0, 2.0, 8.0, 1.0, 3.0, 4.0];
        let group_keys = vec![0, 0, 0, 1, 1, 1];

        func.aggregate_double_group_by(&values, &group_keys, holder.as_mut(), None);

        assert!((holder.get_double_result(0) - 8.0).abs() < f64::EPSILON);
        assert!((holder.get_double_result(1) - 4.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_avg_group_by() {
        let func = AvgAggregationFunction::new();
        let mut holder = func.create_group_by_result_holder(10, 100);

        let values = vec![10.0, 20.0, 30.0, 40.0];
        let group_keys = vec![0, 0, 1, 1];

        func.aggregate_double_group_by(&values, &group_keys, holder.as_mut(), None);

        assert!((holder.get_double_result(0) - 15.0).abs() < f64::EPSILON); // (10+20)/2
        assert!((holder.get_double_result(1) - 35.0).abs() < f64::EPSILON); // (30+40)/2
    }

    #[test]
    fn test_merge_operations() {
        let sum_func = SumAggregationFunction::new();
        assert!((sum_func.merge(10.0, 20.0) - 30.0).abs() < f64::EPSILON);

        let count_func = CountAggregationFunction::new();
        assert!((count_func.merge(5.0, 10.0) - 15.0).abs() < f64::EPSILON);

        let min_func = MinAggregationFunction::new();
        assert!((min_func.merge(5.0, 3.0) - 3.0).abs() < f64::EPSILON);

        let max_func = MaxAggregationFunction::new();
        assert!((max_func.merge(5.0, 8.0) - 8.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_factory() {
        let sum = AggregationFunctionFactory::create(AggregationType::Sum);
        assert_eq!(sum.aggregation_type(), AggregationType::Sum);

        let count = AggregationFunctionFactory::create(AggregationType::Count);
        assert_eq!(count.aggregation_type(), AggregationType::Count);
    }

    #[test]
    fn test_long_values() {
        let func = SumAggregationFunction::new();
        let mut holder = func.create_result_holder();

        let values: Vec<i64> = vec![1, 2, 3, 4, 5];
        func.aggregate_long(&values, holder.as_mut(), None);

        assert!((holder.get_double() - 15.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_empty_values() {
        let func = SumAggregationFunction::new();
        let mut holder = func.create_result_holder();

        let values: Vec<f64> = vec![];
        func.aggregate_double(&values, holder.as_mut(), None);

        assert!((holder.get_double() - 0.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_multiple_aggregations() {
        let func = SumAggregationFunction::new();
        let mut holder = func.create_result_holder();

        // First batch
        let values1 = vec![1.0, 2.0, 3.0];
        func.aggregate_double(&values1, holder.as_mut(), None);

        // Second batch
        let values2 = vec![4.0, 5.0, 6.0];
        func.aggregate_double(&values2, holder.as_mut(), None);

        assert!((holder.get_double() - 21.0).abs() < f64::EPSILON);
    }
}
