//! Aggregation executor
//!
//! Coordinates multiple aggregation functions over data blocks.

use super::functions::*;
use super::result_holders::*;
use super::{AggregationResult, AggregationType};
use roaring::RoaringBitmap;

/// Executor for running multiple aggregation functions
pub struct AggregationExecutor {
    functions: Vec<Box<dyn AggregationFunction>>,
    result_holders: Vec<Box<dyn ResultHolder>>,
}

impl AggregationExecutor {
    /// Creates a new executor with the given aggregation types
    pub fn new(aggregation_types: &[AggregationType]) -> Self {
        let functions: Vec<_> = aggregation_types
            .iter()
            .map(|t| AggregationFunctionFactory::create(*t))
            .collect();

        let result_holders: Vec<_> = functions
            .iter()
            .map(|f| f.create_result_holder())
            .collect();

        Self {
            functions,
            result_holders,
        }
    }

    /// Aggregates a block of double values
    pub fn aggregate_double(&mut self, values: &[f64], null_bitmap: Option<&RoaringBitmap>) {
        for (func, holder) in self.functions.iter().zip(self.result_holders.iter_mut()) {
            func.aggregate_double(values, holder.as_mut(), null_bitmap);
        }
    }

    /// Aggregates a block of long values
    pub fn aggregate_long(&mut self, values: &[i64], null_bitmap: Option<&RoaringBitmap>) {
        for (func, holder) in self.functions.iter().zip(self.result_holders.iter_mut()) {
            func.aggregate_long(values, holder.as_mut(), null_bitmap);
        }
    }

    /// Extracts the final results
    pub fn get_results(&self) -> AggregationResult<Vec<f64>> {
        self.functions
            .iter()
            .zip(self.result_holders.iter())
            .map(|(func, holder)| func.extract_result(holder.as_ref()))
            .collect()
    }

    /// Resets all result holders
    pub fn reset(&mut self) {
        for holder in self.result_holders.iter_mut() {
            holder.reset();
        }
    }

    /// Returns the number of aggregation functions
    pub fn num_functions(&self) -> usize {
        self.functions.len()
    }
}

/// Executor for group-by aggregation
pub struct GroupByAggregationExecutor {
    functions: Vec<Box<dyn AggregationFunction>>,
    result_holders: Vec<Box<dyn GroupByResultHolder>>,
    max_capacity: usize,
}

impl GroupByAggregationExecutor {
    /// Creates a new group-by executor
    pub fn new(
        aggregation_types: &[AggregationType],
        initial_capacity: usize,
        max_capacity: usize,
    ) -> Self {
        let functions: Vec<_> = aggregation_types
            .iter()
            .map(|t| AggregationFunctionFactory::create(*t))
            .collect();

        let result_holders: Vec<_> = functions
            .iter()
            .map(|f| f.create_group_by_result_holder(initial_capacity, max_capacity))
            .collect();

        Self {
            functions,
            result_holders,
            max_capacity,
        }
    }

    /// Ensures capacity for the given upper bound of group keys
    pub fn ensure_capacity(&mut self, capacity: usize) {
        for holder in self.result_holders.iter_mut() {
            holder.ensure_capacity(capacity);
        }
    }

    /// Aggregates a block of double values with group keys
    pub fn aggregate_double(
        &mut self,
        values: &[f64],
        group_keys: &[i32],
        null_bitmap: Option<&RoaringBitmap>,
    ) {
        for (func, holder) in self.functions.iter().zip(self.result_holders.iter_mut()) {
            func.aggregate_double_group_by(values, group_keys, holder.as_mut(), null_bitmap);
        }
    }

    /// Aggregates a block of long values with group keys
    pub fn aggregate_long(
        &mut self,
        values: &[i64],
        group_keys: &[i32],
        null_bitmap: Option<&RoaringBitmap>,
    ) {
        for (func, holder) in self.functions.iter().zip(self.result_holders.iter_mut()) {
            func.aggregate_long_group_by(values, group_keys, holder.as_mut(), null_bitmap);
        }
    }

    /// Gets results for a specific group
    pub fn get_results_for_group(&self, group_key: i32) -> Vec<f64> {
        self.result_holders
            .iter()
            .map(|h| h.get_double_result(group_key))
            .collect()
    }

    /// Gets results for all groups up to the given max key
    pub fn get_all_results(&self, max_group_key: i32) -> Vec<Vec<f64>> {
        (0..=max_group_key)
            .map(|key| self.get_results_for_group(key))
            .collect()
    }

    /// Returns the number of aggregation functions
    pub fn num_functions(&self) -> usize {
        self.functions.len()
    }

    /// Returns the max capacity
    pub fn max_capacity(&self) -> usize {
        self.max_capacity
    }
}

/// Builder for creating aggregation executors
pub struct AggregationExecutorBuilder {
    aggregation_types: Vec<AggregationType>,
    is_group_by: bool,
    initial_capacity: usize,
    max_capacity: usize,
}

impl AggregationExecutorBuilder {
    pub fn new() -> Self {
        Self {
            aggregation_types: Vec::new(),
            is_group_by: false,
            initial_capacity: 1000,
            max_capacity: 100_000,
        }
    }

    pub fn add_aggregation(mut self, aggregation_type: AggregationType) -> Self {
        self.aggregation_types.push(aggregation_type);
        self
    }

    pub fn with_group_by(mut self, initial_capacity: usize, max_capacity: usize) -> Self {
        self.is_group_by = true;
        self.initial_capacity = initial_capacity;
        self.max_capacity = max_capacity;
        self
    }

    pub fn build_simple(self) -> AggregationExecutor {
        AggregationExecutor::new(&self.aggregation_types)
    }

    pub fn build_group_by(self) -> GroupByAggregationExecutor {
        GroupByAggregationExecutor::new(
            &self.aggregation_types,
            self.initial_capacity,
            self.max_capacity,
        )
    }
}

impl Default for AggregationExecutorBuilder {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_simple_executor() {
        let mut executor = AggregationExecutor::new(&[
            AggregationType::Sum,
            AggregationType::Count,
            AggregationType::Min,
            AggregationType::Max,
        ]);

        let values = vec![1.0, 2.0, 3.0, 4.0, 5.0];
        executor.aggregate_double(&values, None);

        let results = executor.get_results().unwrap();
        assert_eq!(results.len(), 4);
        assert!((results[0] - 15.0).abs() < f64::EPSILON); // SUM
        assert!((results[1] - 5.0).abs() < f64::EPSILON); // COUNT
        assert!((results[2] - 1.0).abs() < f64::EPSILON); // MIN
        assert!((results[3] - 5.0).abs() < f64::EPSILON); // MAX
    }

    #[test]
    fn test_group_by_executor() {
        let mut executor = GroupByAggregationExecutor::new(
            &[AggregationType::Sum, AggregationType::Count],
            10,
            100,
        );

        let values = vec![1.0, 2.0, 3.0, 4.0, 5.0, 6.0];
        let group_keys = vec![0, 0, 1, 1, 2, 2];

        executor.aggregate_double(&values, &group_keys, None);

        let results_g0 = executor.get_results_for_group(0);
        assert!((results_g0[0] - 3.0).abs() < f64::EPSILON); // SUM: 1+2
        assert!((results_g0[1] - 2.0).abs() < f64::EPSILON); // COUNT: 2

        let results_g1 = executor.get_results_for_group(1);
        assert!((results_g1[0] - 7.0).abs() < f64::EPSILON); // SUM: 3+4
        assert!((results_g1[1] - 2.0).abs() < f64::EPSILON); // COUNT: 2

        let results_g2 = executor.get_results_for_group(2);
        assert!((results_g2[0] - 11.0).abs() < f64::EPSILON); // SUM: 5+6
        assert!((results_g2[1] - 2.0).abs() < f64::EPSILON); // COUNT: 2
    }

    #[test]
    fn test_executor_builder() {
        let executor = AggregationExecutorBuilder::new()
            .add_aggregation(AggregationType::Sum)
            .add_aggregation(AggregationType::Avg)
            .build_simple();

        assert_eq!(executor.num_functions(), 2);
    }

    #[test]
    fn test_executor_reset() {
        let mut executor = AggregationExecutor::new(&[AggregationType::Sum]);

        let values = vec![1.0, 2.0, 3.0];
        executor.aggregate_double(&values, None);
        assert!((executor.get_results().unwrap()[0] - 6.0).abs() < f64::EPSILON);

        executor.reset();
        assert!((executor.get_results().unwrap()[0] - 0.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_multiple_batches() {
        let mut executor = AggregationExecutor::new(&[AggregationType::Sum]);

        executor.aggregate_double(&[1.0, 2.0], None);
        executor.aggregate_double(&[3.0, 4.0], None);
        executor.aggregate_double(&[5.0], None);

        assert!((executor.get_results().unwrap()[0] - 15.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_group_by_capacity() {
        let mut executor =
            GroupByAggregationExecutor::new(&[AggregationType::Count], 10, 1000);

        // This should work because we ensure capacity
        executor.ensure_capacity(100);

        let values: Vec<f64> = (0..100).map(|i| i as f64).collect();
        let group_keys: Vec<i32> = (0..100).map(|i| i).collect();

        executor.aggregate_double(&values, &group_keys, None);

        // Each group should have count 1
        for i in 0..100 {
            assert_eq!(executor.get_results_for_group(i)[0] as i64, 1);
        }
    }

    #[test]
    fn test_all_aggregations_together() {
        let mut executor = AggregationExecutor::new(&[
            AggregationType::Sum,
            AggregationType::Count,
            AggregationType::Min,
            AggregationType::Max,
            AggregationType::Avg,
        ]);

        let values = vec![10.0, 20.0, 30.0, 40.0, 50.0];
        executor.aggregate_double(&values, None);

        let results = executor.get_results().unwrap();
        assert!((results[0] - 150.0).abs() < f64::EPSILON); // SUM
        assert!((results[1] - 5.0).abs() < f64::EPSILON); // COUNT
        assert!((results[2] - 10.0).abs() < f64::EPSILON); // MIN
        assert!((results[3] - 50.0).abs() < f64::EPSILON); // MAX
        assert!((results[4] - 30.0).abs() < f64::EPSILON); // AVG
    }
}
