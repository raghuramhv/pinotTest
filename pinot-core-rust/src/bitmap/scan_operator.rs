//! Segment scan operator for applying filters during data retrieval.
//!
//! This module provides efficient filtering during segment scans using:
//! - Dictionary-based predicate evaluation for encoded columns
//! - Bitmap operations for combining multiple predicates
//! - Early termination and count optimizations

use super::{
    doc_id_set::{DocIdSet, MatchAllDocIdSet},
    filter_operators::{BitmapFilterOperator, FilterOperator, MatchAllFilterOperator},
};
use crate::dictionary::{Dictionary, DictionaryValue, NULL_VALUE_INDEX};
use ordered_float::OrderedFloat;
use roaring::RoaringBitmap;
use std::sync::Arc;

/// Filter context types matching the Java FilterContext structure
#[derive(Debug, Clone)]
pub enum FilterContext {
    /// Always true constant
    True,
    /// Always false constant
    False,
    /// AND combination of filters
    And(Vec<FilterContext>),
    /// OR combination of filters
    Or(Vec<FilterContext>),
    /// NOT of a filter
    Not(Box<FilterContext>),
    /// A predicate filter
    Predicate(Predicate),
}

/// Predicate types for segment filtering
#[derive(Debug, Clone)]
pub enum Predicate {
    /// Equality: column = value
    Eq { column: String, value: PredicateValue },
    /// Inequality: column != value
    NotEq { column: String, value: PredicateValue },
    /// Less than: column < value
    Lt { column: String, value: PredicateValue },
    /// Less than or equal: column <= value
    Le { column: String, value: PredicateValue },
    /// Greater than: column > value
    Gt { column: String, value: PredicateValue },
    /// Greater than or equal: column >= value
    Ge { column: String, value: PredicateValue },
    /// Between: lower <= column <= upper
    Between {
        column: String,
        lower: PredicateValue,
        upper: PredicateValue,
        lower_inclusive: bool,
        upper_inclusive: bool,
    },
    /// IN: column IN (values)
    In { column: String, values: Vec<PredicateValue> },
    /// NOT IN: column NOT IN (values)
    NotIn { column: String, values: Vec<PredicateValue> },
    /// IS NULL: column IS NULL
    IsNull { column: String },
    /// IS NOT NULL: column IS NOT NULL
    IsNotNull { column: String },
    /// LIKE pattern matching
    Like { column: String, pattern: String },
    /// Regular expression matching
    Regexp { column: String, pattern: String },
}

/// Predicate values for comparison
#[derive(Debug, Clone, PartialEq)]
pub enum PredicateValue {
    Int(i32),
    Long(i64),
    Float(f32),
    Double(f64),
    String(String),
    Bytes(Vec<u8>),
}

impl PredicateValue {
    /// Get as i64 if possible
    pub fn as_long(&self) -> Option<i64> {
        match self {
            PredicateValue::Int(v) => Some(*v as i64),
            PredicateValue::Long(v) => Some(*v),
            _ => None,
        }
    }

    /// Get as f64 if possible
    pub fn as_double(&self) -> Option<f64> {
        match self {
            PredicateValue::Int(v) => Some(*v as f64),
            PredicateValue::Long(v) => Some(*v as f64),
            PredicateValue::Float(v) => Some(*v as f64),
            PredicateValue::Double(v) => Some(*v),
            _ => None,
        }
    }

    /// Get as string reference if possible
    pub fn as_str(&self) -> Option<&str> {
        match self {
            PredicateValue::String(s) => Some(s.as_str()),
            _ => None,
        }
    }
}

impl From<i32> for PredicateValue {
    fn from(v: i32) -> Self {
        PredicateValue::Int(v)
    }
}

impl From<i64> for PredicateValue {
    fn from(v: i64) -> Self {
        PredicateValue::Long(v)
    }
}

impl From<f32> for PredicateValue {
    fn from(v: f32) -> Self {
        PredicateValue::Float(v)
    }
}

impl From<f64> for PredicateValue {
    fn from(v: f64) -> Self {
        PredicateValue::Double(v)
    }
}

impl From<String> for PredicateValue {
    fn from(v: String) -> Self {
        PredicateValue::String(v)
    }
}

impl From<&str> for PredicateValue {
    fn from(v: &str) -> Self {
        PredicateValue::String(v.to_string())
    }
}

/// Result of building a filter from predicates
pub struct FilterBuildResult {
    /// The built filter operator
    pub filter: Box<dyn FilterOperator>,
    /// Columns that need to be scanned
    pub scan_columns: Vec<String>,
    /// Whether the filter is always true
    pub always_true: bool,
    /// Whether the filter is always false
    pub always_false: bool,
}

/// Builder for segment scan filters
pub struct ScanFilterBuilder<D: Dictionary> {
    /// Dictionary provider for each column
    dictionaries: std::collections::HashMap<String, Arc<D>>,
    /// Total number of documents
    num_docs: i32,
}

impl<D: Dictionary> ScanFilterBuilder<D> {
    /// Create a new scan filter builder
    pub fn new(num_docs: i32) -> Self {
        Self {
            dictionaries: std::collections::HashMap::new(),
            num_docs,
        }
    }

    /// Add a dictionary for a column
    pub fn with_dictionary(mut self, column: impl Into<String>, dictionary: Arc<D>) -> Self {
        self.dictionaries.insert(column.into(), dictionary);
        self
    }

    /// Build a filter from a filter context
    pub fn build(&self, context: &FilterContext) -> FilterBuildResult {
        match context {
            FilterContext::True => FilterBuildResult {
                filter: Box::new(MatchAllFilterOperator::new(self.num_docs)),
                scan_columns: vec![],
                always_true: true,
                always_false: false,
            },
            FilterContext::False => FilterBuildResult {
                filter: Box::new(super::filter_operators::EmptyFilterOperator::new(
                    self.num_docs,
                )),
                scan_columns: vec![],
                always_true: false,
                always_false: true,
            },
            FilterContext::And(children) => self.build_and(children),
            FilterContext::Or(children) => self.build_or(children),
            FilterContext::Not(inner) => self.build_not(inner),
            FilterContext::Predicate(pred) => self.build_predicate(pred),
        }
    }

    fn build_and(&self, children: &[FilterContext]) -> FilterBuildResult {
        let mut filters: Vec<Box<dyn FilterOperator>> = Vec::new();
        let mut all_columns = Vec::new();

        for child in children {
            let result = self.build(child);
            if result.always_false {
                return FilterBuildResult {
                    filter: Box::new(super::filter_operators::EmptyFilterOperator::new(
                        self.num_docs,
                    )),
                    scan_columns: vec![],
                    always_true: false,
                    always_false: true,
                };
            }
            if !result.always_true {
                filters.push(result.filter);
                all_columns.extend(result.scan_columns);
            }
        }

        if filters.is_empty() {
            return FilterBuildResult {
                filter: Box::new(MatchAllFilterOperator::new(self.num_docs)),
                scan_columns: vec![],
                always_true: true,
                always_false: false,
            };
        }

        if filters.len() == 1 {
            return FilterBuildResult {
                filter: filters.into_iter().next().unwrap(),
                scan_columns: all_columns,
                always_true: false,
                always_false: false,
            };
        }

        FilterBuildResult {
            filter: Box::new(super::filter_operators::AndFilterOperator::new(
                filters,
                self.num_docs,
            )),
            scan_columns: all_columns,
            always_true: false,
            always_false: false,
        }
    }

    fn build_or(&self, children: &[FilterContext]) -> FilterBuildResult {
        let mut filters: Vec<Box<dyn FilterOperator>> = Vec::new();
        let mut all_columns = Vec::new();

        for child in children {
            let result = self.build(child);
            if result.always_true {
                return FilterBuildResult {
                    filter: Box::new(MatchAllFilterOperator::new(self.num_docs)),
                    scan_columns: vec![],
                    always_true: true,
                    always_false: false,
                };
            }
            if !result.always_false {
                filters.push(result.filter);
                all_columns.extend(result.scan_columns);
            }
        }

        if filters.is_empty() {
            return FilterBuildResult {
                filter: Box::new(super::filter_operators::EmptyFilterOperator::new(
                    self.num_docs,
                )),
                scan_columns: vec![],
                always_true: false,
                always_false: true,
            };
        }

        if filters.len() == 1 {
            return FilterBuildResult {
                filter: filters.into_iter().next().unwrap(),
                scan_columns: all_columns,
                always_true: false,
                always_false: false,
            };
        }

        FilterBuildResult {
            filter: Box::new(super::filter_operators::OrFilterOperator::new(
                filters,
                self.num_docs,
            )),
            scan_columns: all_columns,
            always_true: false,
            always_false: false,
        }
    }

    fn build_not(&self, inner: &FilterContext) -> FilterBuildResult {
        let inner_result = self.build(inner);

        if inner_result.always_true {
            return FilterBuildResult {
                filter: Box::new(super::filter_operators::EmptyFilterOperator::new(
                    self.num_docs,
                )),
                scan_columns: vec![],
                always_true: false,
                always_false: true,
            };
        }

        if inner_result.always_false {
            return FilterBuildResult {
                filter: Box::new(MatchAllFilterOperator::new(self.num_docs)),
                scan_columns: vec![],
                always_true: true,
                always_false: false,
            };
        }

        FilterBuildResult {
            filter: Box::new(super::filter_operators::NotFilterOperator::new(
                inner_result.filter,
                self.num_docs,
            )),
            scan_columns: inner_result.scan_columns,
            always_true: false,
            always_false: false,
        }
    }

    fn build_predicate(&self, pred: &Predicate) -> FilterBuildResult {
        match pred {
            Predicate::Eq { column, value } => self.build_eq_predicate(column, value),
            Predicate::NotEq { column, value } => self.build_not_eq_predicate(column, value),
            Predicate::Lt { column, value } => self.build_range_predicate(
                column,
                None,
                Some(value.clone()),
                true,
                false,
            ),
            Predicate::Le { column, value } => self.build_range_predicate(
                column,
                None,
                Some(value.clone()),
                true,
                true,
            ),
            Predicate::Gt { column, value } => self.build_range_predicate(
                column,
                Some(value.clone()),
                None,
                false,
                true,
            ),
            Predicate::Ge { column, value } => self.build_range_predicate(
                column,
                Some(value.clone()),
                None,
                true,
                true,
            ),
            Predicate::Between {
                column,
                lower,
                upper,
                lower_inclusive,
                upper_inclusive,
            } => self.build_range_predicate(
                column,
                Some(lower.clone()),
                Some(upper.clone()),
                *lower_inclusive,
                *upper_inclusive,
            ),
            Predicate::In { column, values } => self.build_in_predicate(column, values, false),
            Predicate::NotIn { column, values } => self.build_in_predicate(column, values, true),
            Predicate::IsNull { column } => self.build_null_predicate(column, true),
            Predicate::IsNotNull { column } => self.build_null_predicate(column, false),
            Predicate::Like { column, pattern } => self.build_like_predicate(column, pattern),
            Predicate::Regexp { column, pattern } => self.build_regexp_predicate(column, pattern),
        }
    }

    fn build_eq_predicate(&self, column: &str, value: &PredicateValue) -> FilterBuildResult {
        let dict = match self.dictionaries.get(column) {
            Some(d) => d,
            None => {
                // No dictionary - need to scan
                return FilterBuildResult {
                    filter: Box::new(MatchAllFilterOperator::new(self.num_docs)),
                    scan_columns: vec![column.to_string()],
                    always_true: false,
                    always_false: false,
                };
            }
        };

        let dict_id = self.lookup_value(dict, value);

        if dict_id == NULL_VALUE_INDEX {
            // Value not in dictionary - no matches
            FilterBuildResult {
                filter: Box::new(super::filter_operators::EmptyFilterOperator::new(
                    self.num_docs,
                )),
                scan_columns: vec![],
                always_true: false,
                always_false: true,
            }
        } else {
            FilterBuildResult {
                filter: Box::new(BitmapFilterOperator::new(
                    RoaringBitmap::from_iter([dict_id as u32]),
                    self.num_docs,
                    false,
                )),
                scan_columns: vec![column.to_string()],
                always_true: false,
                always_false: false,
            }
        }
    }

    fn build_not_eq_predicate(&self, column: &str, value: &PredicateValue) -> FilterBuildResult {
        let dict = match self.dictionaries.get(column) {
            Some(d) => d,
            None => {
                return FilterBuildResult {
                    filter: Box::new(MatchAllFilterOperator::new(self.num_docs)),
                    scan_columns: vec![column.to_string()],
                    always_true: false,
                    always_false: false,
                };
            }
        };

        let dict_id = self.lookup_value(dict, value);

        if dict_id == NULL_VALUE_INDEX {
            // Value not in dictionary - all match
            FilterBuildResult {
                filter: Box::new(MatchAllFilterOperator::new(self.num_docs)),
                scan_columns: vec![],
                always_true: true,
                always_false: false,
            }
        } else {
            FilterBuildResult {
                filter: Box::new(BitmapFilterOperator::new(
                    RoaringBitmap::from_iter([dict_id as u32]),
                    self.num_docs,
                    true, // exclusive
                )),
                scan_columns: vec![column.to_string()],
                always_true: false,
                always_false: false,
            }
        }
    }

    fn build_range_predicate(
        &self,
        column: &str,
        lower: Option<PredicateValue>,
        upper: Option<PredicateValue>,
        lower_inclusive: bool,
        upper_inclusive: bool,
    ) -> FilterBuildResult {
        let dict = match self.dictionaries.get(column) {
            Some(d) => d,
            None => {
                return FilterBuildResult {
                    filter: Box::new(MatchAllFilterOperator::new(self.num_docs)),
                    scan_columns: vec![column.to_string()],
                    always_true: false,
                    always_false: false,
                };
            }
        };

        // Get insertion indices for bounds
        let min_dict_id = match &lower {
            Some(val) => {
                let id = self.lookup_insertion_index(dict, val);
                if lower_inclusive {
                    id
                } else {
                    id + 1
                }
            }
            None => 0,
        };

        let max_dict_id = match &upper {
            Some(val) => {
                let id = self.lookup_insertion_index(dict, val);
                if upper_inclusive {
                    id + 1
                } else {
                    id
                }
            }
            None => dict.len() as i32,
        };

        if min_dict_id >= max_dict_id {
            return FilterBuildResult {
                filter: Box::new(super::filter_operators::EmptyFilterOperator::new(
                    self.num_docs,
                )),
                scan_columns: vec![],
                always_true: false,
                always_false: true,
            };
        }

        let mut bitmap = RoaringBitmap::new();
        for id in min_dict_id..max_dict_id {
            bitmap.insert(id as u32);
        }

        FilterBuildResult {
            filter: Box::new(BitmapFilterOperator::new(bitmap, self.num_docs, false)),
            scan_columns: vec![column.to_string()],
            always_true: false,
            always_false: false,
        }
    }

    fn build_in_predicate(
        &self,
        column: &str,
        values: &[PredicateValue],
        negate: bool,
    ) -> FilterBuildResult {
        let dict = match self.dictionaries.get(column) {
            Some(d) => d,
            None => {
                return FilterBuildResult {
                    filter: Box::new(MatchAllFilterOperator::new(self.num_docs)),
                    scan_columns: vec![column.to_string()],
                    always_true: false,
                    always_false: false,
                };
            }
        };

        let mut bitmap = RoaringBitmap::new();
        for value in values {
            let dict_id = self.lookup_value(dict, value);
            if dict_id != NULL_VALUE_INDEX {
                bitmap.insert(dict_id as u32);
            }
        }

        if bitmap.is_empty() {
            if negate {
                // NOT IN empty set = all match
                return FilterBuildResult {
                    filter: Box::new(MatchAllFilterOperator::new(self.num_docs)),
                    scan_columns: vec![],
                    always_true: true,
                    always_false: false,
                };
            } else {
                // IN empty set = no match
                return FilterBuildResult {
                    filter: Box::new(super::filter_operators::EmptyFilterOperator::new(
                        self.num_docs,
                    )),
                    scan_columns: vec![],
                    always_true: false,
                    always_false: true,
                };
            }
        }

        FilterBuildResult {
            filter: Box::new(BitmapFilterOperator::new(bitmap, self.num_docs, negate)),
            scan_columns: vec![column.to_string()],
            always_true: false,
            always_false: false,
        }
    }

    fn build_null_predicate(&self, column: &str, _is_null: bool) -> FilterBuildResult {
        // For null predicates, we need to check the null bitmap
        // This typically requires scanning the null vector
        FilterBuildResult {
            filter: Box::new(MatchAllFilterOperator::new(self.num_docs)),
            scan_columns: vec![column.to_string()],
            always_true: false,
            always_false: false,
        }
    }

    fn build_like_predicate(&self, column: &str, _pattern: &str) -> FilterBuildResult {
        // LIKE predicates require scanning string values
        // Could potentially use prefix optimization for patterns like "abc%"
        FilterBuildResult {
            filter: Box::new(MatchAllFilterOperator::new(self.num_docs)),
            scan_columns: vec![column.to_string()],
            always_true: false,
            always_false: false,
        }
    }

    fn build_regexp_predicate(&self, column: &str, _pattern: &str) -> FilterBuildResult {
        // Regexp predicates require scanning
        FilterBuildResult {
            filter: Box::new(MatchAllFilterOperator::new(self.num_docs)),
            scan_columns: vec![column.to_string()],
            always_true: false,
            always_false: false,
        }
    }

    /// Look up a value in the dictionary and return its ID
    fn lookup_value(&self, dict: &D, value: &PredicateValue) -> i32 {
        match value {
            PredicateValue::Int(v) => dict.index_of_int(*v),
            PredicateValue::Long(v) => dict.index_of_long(*v),
            PredicateValue::String(v) => dict.index_of_string(v),
            PredicateValue::Bytes(v) => dict.index_of_bytes(v),
            PredicateValue::Float(_) | PredicateValue::Double(_) => {
                // Float/double lookup requires special handling
                NULL_VALUE_INDEX
            }
        }
    }

    /// Look up the insertion index for a value (for range queries)
    fn lookup_insertion_index(&self, dict: &D, value: &PredicateValue) -> i32 {
        let dict_value = match value {
            PredicateValue::Int(v) => DictionaryValue::Int(*v),
            PredicateValue::Long(v) => DictionaryValue::Long(*v),
            PredicateValue::Float(v) => DictionaryValue::Float(OrderedFloat(*v)),
            PredicateValue::Double(v) => DictionaryValue::Double(OrderedFloat(*v)),
            PredicateValue::String(v) => DictionaryValue::String(v.clone()),
            PredicateValue::Bytes(v) => DictionaryValue::Bytes(v.clone()),
        };
        dict.insertion_index_of(&dict_value)
    }
}

/// Scan operator for filtering segment data
pub struct ScanOperator<D: Dictionary> {
    /// Filter builder
    builder: ScanFilterBuilder<D>,
    /// The built filter
    filter: Option<FilterBuildResult>,
}

impl<D: Dictionary> ScanOperator<D> {
    /// Create a new scan operator
    pub fn new(num_docs: i32) -> Self {
        Self {
            builder: ScanFilterBuilder::new(num_docs),
            filter: None,
        }
    }

    /// Add a dictionary for a column
    pub fn with_dictionary(mut self, column: impl Into<String>, dictionary: Arc<D>) -> Self {
        self.builder = self.builder.with_dictionary(column, dictionary);
        self
    }

    /// Set the filter context
    pub fn with_filter(mut self, context: &FilterContext) -> Self {
        self.filter = Some(self.builder.build(context));
        self
    }

    /// Get matching document IDs
    pub fn get_matching_doc_ids(&self) -> Box<dyn DocIdSet> {
        match &self.filter {
            Some(result) => result.filter.get_matching_doc_ids(),
            None => Box::new(MatchAllDocIdSet::new(self.builder.num_docs)),
        }
    }

    /// Get the count of matching documents if it can be optimized
    pub fn get_matching_count(&self) -> Option<u64> {
        match &self.filter {
            Some(result) if result.filter.can_optimize_count() => {
                result.filter.get_matching_count()
            }
            _ => None,
        }
    }

    /// Check if the filter is always true
    pub fn is_always_true(&self) -> bool {
        self.filter.as_ref().map(|r| r.always_true).unwrap_or(true)
    }

    /// Check if the filter is always false
    pub fn is_always_false(&self) -> bool {
        self.filter.as_ref().map(|r| r.always_false).unwrap_or(false)
    }

    /// Get columns that need to be scanned
    pub fn scan_columns(&self) -> &[String] {
        match &self.filter {
            Some(result) => &result.scan_columns,
            None => &[],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dictionary::{OnHeapIntDictionary, OnHeapStringDictionary};

    #[test]
    fn test_filter_context_true() {
        let builder: ScanFilterBuilder<OnHeapIntDictionary> = ScanFilterBuilder::new(100);
        let result = builder.build(&FilterContext::True);
        assert!(result.always_true);
        assert!(!result.always_false);
    }

    #[test]
    fn test_filter_context_false() {
        let builder: ScanFilterBuilder<OnHeapIntDictionary> = ScanFilterBuilder::new(100);
        let result = builder.build(&FilterContext::False);
        assert!(!result.always_true);
        assert!(result.always_false);
    }

    #[test]
    fn test_eq_predicate_found() {
        // Create an immutable int dictionary with sorted values
        let dict = OnHeapIntDictionary::from_unsorted(vec![10, 20, 30]);

        let builder = ScanFilterBuilder::new(100)
            .with_dictionary("col", Arc::new(dict));

        let ctx = FilterContext::Predicate(Predicate::Eq {
            column: "col".to_string(),
            value: PredicateValue::Int(20),
        });

        let result = builder.build(&ctx);
        assert!(!result.always_true);
        assert!(!result.always_false);
    }

    #[test]
    fn test_eq_predicate_not_found() {
        let dict = OnHeapIntDictionary::from_unsorted(vec![10, 20]);

        let builder = ScanFilterBuilder::new(100)
            .with_dictionary("col", Arc::new(dict));

        let ctx = FilterContext::Predicate(Predicate::Eq {
            column: "col".to_string(),
            value: PredicateValue::Int(99), // Not in dictionary
        });

        let result = builder.build(&ctx);
        assert!(!result.always_true);
        assert!(result.always_false);
    }

    #[test]
    fn test_not_eq_predicate_not_found() {
        let dict = OnHeapIntDictionary::from_unsorted(vec![10, 20]);

        let builder = ScanFilterBuilder::new(100)
            .with_dictionary("col", Arc::new(dict));

        let ctx = FilterContext::Predicate(Predicate::NotEq {
            column: "col".to_string(),
            value: PredicateValue::Int(99), // Not in dictionary
        });

        let result = builder.build(&ctx);
        assert!(result.always_true); // All rows match since value not in dict
        assert!(!result.always_false);
    }

    #[test]
    fn test_in_predicate() {
        let dict = OnHeapIntDictionary::from_unsorted(vec![10, 20, 30]);

        let builder = ScanFilterBuilder::new(100)
            .with_dictionary("col", Arc::new(dict));

        let ctx = FilterContext::Predicate(Predicate::In {
            column: "col".to_string(),
            values: vec![PredicateValue::Int(10), PredicateValue::Int(30)],
        });

        let result = builder.build(&ctx);
        assert!(!result.always_true);
        assert!(!result.always_false);
    }

    #[test]
    fn test_in_predicate_empty() {
        let dict = OnHeapIntDictionary::from_unsorted(vec![10, 20]);

        let builder = ScanFilterBuilder::new(100)
            .with_dictionary("col", Arc::new(dict));

        let ctx = FilterContext::Predicate(Predicate::In {
            column: "col".to_string(),
            values: vec![PredicateValue::Int(99)], // Not in dictionary
        });

        let result = builder.build(&ctx);
        assert!(!result.always_true);
        assert!(result.always_false);
    }

    #[test]
    fn test_and_with_false() {
        let dict = OnHeapIntDictionary::from_unsorted(vec![10]);

        let builder = ScanFilterBuilder::new(100)
            .with_dictionary("col", Arc::new(dict));

        let ctx = FilterContext::And(vec![
            FilterContext::True,
            FilterContext::False,
        ]);

        let result = builder.build(&ctx);
        assert!(!result.always_true);
        assert!(result.always_false);
    }

    #[test]
    fn test_or_with_true() {
        let builder: ScanFilterBuilder<OnHeapIntDictionary> = ScanFilterBuilder::new(100);

        let ctx = FilterContext::Or(vec![
            FilterContext::False,
            FilterContext::True,
        ]);

        let result = builder.build(&ctx);
        assert!(result.always_true);
        assert!(!result.always_false);
    }

    #[test]
    fn test_not_true() {
        let builder: ScanFilterBuilder<OnHeapIntDictionary> = ScanFilterBuilder::new(100);

        let ctx = FilterContext::Not(Box::new(FilterContext::True));

        let result = builder.build(&ctx);
        assert!(!result.always_true);
        assert!(result.always_false);
    }

    #[test]
    fn test_not_false() {
        let builder: ScanFilterBuilder<OnHeapIntDictionary> = ScanFilterBuilder::new(100);

        let ctx = FilterContext::Not(Box::new(FilterContext::False));

        let result = builder.build(&ctx);
        assert!(result.always_true);
        assert!(!result.always_false);
    }

    #[test]
    fn test_scan_operator() {
        let dict = OnHeapIntDictionary::from_unsorted(vec![10, 20, 30]);

        let ctx = FilterContext::Predicate(Predicate::Eq {
            column: "col".to_string(),
            value: PredicateValue::Int(20),
        });

        let scan: ScanOperator<OnHeapIntDictionary> = ScanOperator::new(100)
            .with_dictionary("col", Arc::new(dict))
            .with_filter(&ctx);

        assert!(!scan.is_always_true());
        assert!(!scan.is_always_false());
        assert!(!scan.scan_columns().is_empty());
    }

    #[test]
    fn test_predicate_value_conversions() {
        let v: PredicateValue = 42i32.into();
        assert_eq!(v.as_long(), Some(42));
        assert_eq!(v.as_double(), Some(42.0));

        let v: PredicateValue = "hello".into();
        assert_eq!(v.as_str(), Some("hello"));
        assert_eq!(v.as_long(), None);
    }

    #[test]
    fn test_string_predicate() {
        let dict = OnHeapStringDictionary::from_unsorted(vec![
            "apple".to_string(),
            "banana".to_string(),
            "cherry".to_string(),
        ]);

        let builder = ScanFilterBuilder::new(100)
            .with_dictionary("fruit", Arc::new(dict));

        let ctx = FilterContext::Predicate(Predicate::Eq {
            column: "fruit".to_string(),
            value: PredicateValue::String("banana".to_string()),
        });

        let result = builder.build(&ctx);
        assert!(!result.always_true);
        assert!(!result.always_false);
    }

    #[test]
    fn test_complex_filter() {
        let dict = OnHeapIntDictionary::from_unsorted(vec![1, 2, 3, 4, 5]);

        let builder = ScanFilterBuilder::new(100)
            .with_dictionary("id", Arc::new(dict));

        // (id = 1 OR id = 5) AND id != 3
        let ctx = FilterContext::And(vec![
            FilterContext::Or(vec![
                FilterContext::Predicate(Predicate::Eq {
                    column: "id".to_string(),
                    value: PredicateValue::Int(1),
                }),
                FilterContext::Predicate(Predicate::Eq {
                    column: "id".to_string(),
                    value: PredicateValue::Int(5),
                }),
            ]),
            FilterContext::Predicate(Predicate::NotEq {
                column: "id".to_string(),
                value: PredicateValue::Int(3),
            }),
        ]);

        let result = builder.build(&ctx);
        assert!(!result.always_true);
        assert!(!result.always_false);
    }
}
