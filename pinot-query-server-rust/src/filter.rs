//! Filter expression evaluation for query operators.
//!
//! This module provides:
//! - Structured filter predicates (Eq, NotEq, In, Range, etc.)
//! - Expression evaluation on columnar data blocks
//! - Boolean combinators (And, Or, Not)
//! - Null handling semantics

use crate::block::{ColumnData, ColumnType, DataBlock};
use std::cmp::Ordering;
use std::collections::HashSet;
use std::sync::Arc;

/// Result type for filter operations
pub type FilterResult<T> = Result<T, FilterError>;

/// Errors that can occur during filter evaluation
#[derive(Debug, Clone)]
pub enum FilterError {
    /// Column not found in schema
    ColumnNotFound { name: String },
    /// Type mismatch during comparison
    TypeMismatch { expected: ColumnType, actual: ColumnType },
    /// Index out of bounds
    IndexOutOfBounds { index: usize, len: usize },
    /// Invalid predicate configuration
    InvalidPredicate { message: String },
}

impl std::fmt::Display for FilterError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FilterError::ColumnNotFound { name } => {
                write!(f, "Column not found: {}", name)
            }
            FilterError::TypeMismatch { expected, actual } => {
                write!(f, "Type mismatch: expected {:?}, got {:?}", expected, actual)
            }
            FilterError::IndexOutOfBounds { index, len } => {
                write!(f, "Index {} out of bounds (len={})", index, len)
            }
            FilterError::InvalidPredicate { message } => {
                write!(f, "Invalid predicate: {}", message)
            }
        }
    }
}

impl std::error::Error for FilterError {}

/// A scalar value for comparison
#[derive(Debug, Clone, PartialEq)]
pub enum ScalarValue {
    Null,
    Int(i32),
    Long(i64),
    Float(f32),
    Double(f64),
    String(String),
    Bytes(Vec<u8>),
    Boolean(bool),
}

impl ScalarValue {
    /// Convert to i64 if possible
    pub fn as_long(&self) -> Option<i64> {
        match self {
            ScalarValue::Int(v) => Some(*v as i64),
            ScalarValue::Long(v) => Some(*v),
            _ => None,
        }
    }

    /// Convert to f64 if possible
    pub fn as_double(&self) -> Option<f64> {
        match self {
            ScalarValue::Int(v) => Some(*v as f64),
            ScalarValue::Long(v) => Some(*v as f64),
            ScalarValue::Float(v) => Some(*v as f64),
            ScalarValue::Double(v) => Some(*v),
            _ => None,
        }
    }

    /// Get string value if possible
    pub fn as_str(&self) -> Option<&str> {
        match self {
            ScalarValue::String(s) => Some(s.as_str()),
            _ => None,
        }
    }

    /// Check if value is null
    pub fn is_null(&self) -> bool {
        matches!(self, ScalarValue::Null)
    }
}

impl From<i32> for ScalarValue {
    fn from(v: i32) -> Self {
        ScalarValue::Int(v)
    }
}

impl From<i64> for ScalarValue {
    fn from(v: i64) -> Self {
        ScalarValue::Long(v)
    }
}

impl From<f32> for ScalarValue {
    fn from(v: f32) -> Self {
        ScalarValue::Float(v)
    }
}

impl From<f64> for ScalarValue {
    fn from(v: f64) -> Self {
        ScalarValue::Double(v)
    }
}

impl From<String> for ScalarValue {
    fn from(v: String) -> Self {
        ScalarValue::String(v)
    }
}

impl From<&str> for ScalarValue {
    fn from(v: &str) -> Self {
        ScalarValue::String(v.to_string())
    }
}

impl From<bool> for ScalarValue {
    fn from(v: bool) -> Self {
        ScalarValue::Boolean(v)
    }
}

impl From<Vec<u8>> for ScalarValue {
    fn from(v: Vec<u8>) -> Self {
        ScalarValue::Bytes(v)
    }
}

/// Range bounds for range predicates
#[derive(Debug, Clone)]
pub struct RangeBounds {
    /// Lower bound (None = unbounded)
    pub lower: Option<ScalarValue>,
    /// Upper bound (None = unbounded)
    pub upper: Option<ScalarValue>,
    /// Whether lower bound is inclusive
    pub lower_inclusive: bool,
    /// Whether upper bound is inclusive
    pub upper_inclusive: bool,
}

impl RangeBounds {
    pub fn new(
        lower: Option<ScalarValue>,
        upper: Option<ScalarValue>,
        lower_inclusive: bool,
        upper_inclusive: bool,
    ) -> Self {
        Self {
            lower,
            upper,
            lower_inclusive,
            upper_inclusive,
        }
    }

    pub fn less_than(value: ScalarValue) -> Self {
        Self::new(None, Some(value), true, false)
    }

    pub fn less_than_or_equal(value: ScalarValue) -> Self {
        Self::new(None, Some(value), true, true)
    }

    pub fn greater_than(value: ScalarValue) -> Self {
        Self::new(Some(value), None, false, true)
    }

    pub fn greater_than_or_equal(value: ScalarValue) -> Self {
        Self::new(Some(value), None, true, true)
    }

    pub fn between(lower: ScalarValue, upper: ScalarValue) -> Self {
        Self::new(Some(lower), Some(upper), true, true)
    }
}

/// Filter predicate types
#[derive(Debug, Clone)]
pub enum FilterPredicate {
    /// Always true
    True,
    /// Always false
    False,
    /// Equality: column = value
    Eq {
        column: String,
        value: ScalarValue,
    },
    /// Inequality: column != value
    NotEq {
        column: String,
        value: ScalarValue,
    },
    /// Less than: column < value
    Lt {
        column: String,
        value: ScalarValue,
    },
    /// Less than or equal: column <= value
    Le {
        column: String,
        value: ScalarValue,
    },
    /// Greater than: column > value
    Gt {
        column: String,
        value: ScalarValue,
    },
    /// Greater than or equal: column >= value
    Ge {
        column: String,
        value: ScalarValue,
    },
    /// Range: lower <= column <= upper (with optional inclusive bounds)
    Range {
        column: String,
        bounds: RangeBounds,
    },
    /// IN: column IN (value1, value2, ...)
    In {
        column: String,
        values: Vec<ScalarValue>,
    },
    /// NOT IN: column NOT IN (value1, value2, ...)
    NotIn {
        column: String,
        values: Vec<ScalarValue>,
    },
    /// IS NULL: column IS NULL
    IsNull {
        column: String,
    },
    /// IS NOT NULL: column IS NOT NULL
    IsNotNull {
        column: String,
    },
    /// LIKE pattern matching
    Like {
        column: String,
        pattern: String,
        /// Escape character
        escape: Option<char>,
    },
    /// Regular expression matching
    Regexp {
        column: String,
        pattern: String,
    },
    /// Boolean AND of predicates
    And(Vec<FilterPredicate>),
    /// Boolean OR of predicates
    Or(Vec<FilterPredicate>),
    /// Boolean NOT of predicate
    Not(Box<FilterPredicate>),
}

impl FilterPredicate {
    /// Create an equality predicate
    pub fn eq(column: impl Into<String>, value: impl Into<ScalarValue>) -> Self {
        FilterPredicate::Eq {
            column: column.into(),
            value: value.into(),
        }
    }

    /// Create an inequality predicate
    pub fn not_eq(column: impl Into<String>, value: impl Into<ScalarValue>) -> Self {
        FilterPredicate::NotEq {
            column: column.into(),
            value: value.into(),
        }
    }

    /// Create a less-than predicate
    pub fn lt(column: impl Into<String>, value: impl Into<ScalarValue>) -> Self {
        FilterPredicate::Lt {
            column: column.into(),
            value: value.into(),
        }
    }

    /// Create a less-than-or-equal predicate
    pub fn le(column: impl Into<String>, value: impl Into<ScalarValue>) -> Self {
        FilterPredicate::Le {
            column: column.into(),
            value: value.into(),
        }
    }

    /// Create a greater-than predicate
    pub fn gt(column: impl Into<String>, value: impl Into<ScalarValue>) -> Self {
        FilterPredicate::Gt {
            column: column.into(),
            value: value.into(),
        }
    }

    /// Create a greater-than-or-equal predicate
    pub fn ge(column: impl Into<String>, value: impl Into<ScalarValue>) -> Self {
        FilterPredicate::Ge {
            column: column.into(),
            value: value.into(),
        }
    }

    /// Create a BETWEEN predicate (inclusive on both ends)
    pub fn between(
        column: impl Into<String>,
        lower: impl Into<ScalarValue>,
        upper: impl Into<ScalarValue>,
    ) -> Self {
        FilterPredicate::Range {
            column: column.into(),
            bounds: RangeBounds::between(lower.into(), upper.into()),
        }
    }

    /// Create an IN predicate
    pub fn in_list<S: Into<String>, V: Into<ScalarValue>>(
        column: S,
        values: impl IntoIterator<Item = V>,
    ) -> Self {
        FilterPredicate::In {
            column: column.into(),
            values: values.into_iter().map(|v| v.into()).collect(),
        }
    }

    /// Create a NOT IN predicate
    pub fn not_in_list<S: Into<String>, V: Into<ScalarValue>>(
        column: S,
        values: impl IntoIterator<Item = V>,
    ) -> Self {
        FilterPredicate::NotIn {
            column: column.into(),
            values: values.into_iter().map(|v| v.into()).collect(),
        }
    }

    /// Create an IS NULL predicate
    pub fn is_null(column: impl Into<String>) -> Self {
        FilterPredicate::IsNull {
            column: column.into(),
        }
    }

    /// Create an IS NOT NULL predicate
    pub fn is_not_null(column: impl Into<String>) -> Self {
        FilterPredicate::IsNotNull {
            column: column.into(),
        }
    }

    /// Create a LIKE predicate
    pub fn like(column: impl Into<String>, pattern: impl Into<String>) -> Self {
        FilterPredicate::Like {
            column: column.into(),
            pattern: pattern.into(),
            escape: None,
        }
    }

    /// Create a REGEXP predicate
    pub fn regexp(column: impl Into<String>, pattern: impl Into<String>) -> Self {
        FilterPredicate::Regexp {
            column: column.into(),
            pattern: pattern.into(),
        }
    }

    /// Create an AND predicate
    pub fn and(predicates: Vec<FilterPredicate>) -> Self {
        // Optimize: flatten nested ANDs and remove True
        let mut flattened = Vec::new();
        for p in predicates {
            match p {
                FilterPredicate::True => continue,
                FilterPredicate::False => return FilterPredicate::False,
                FilterPredicate::And(children) => flattened.extend(children),
                other => flattened.push(other),
            }
        }
        match flattened.len() {
            0 => FilterPredicate::True,
            1 => flattened.into_iter().next().unwrap(),
            _ => FilterPredicate::And(flattened),
        }
    }

    /// Create an OR predicate
    pub fn or(predicates: Vec<FilterPredicate>) -> Self {
        // Optimize: flatten nested ORs and remove False
        let mut flattened = Vec::new();
        for p in predicates {
            match p {
                FilterPredicate::False => continue,
                FilterPredicate::True => return FilterPredicate::True,
                FilterPredicate::Or(children) => flattened.extend(children),
                other => flattened.push(other),
            }
        }
        match flattened.len() {
            0 => FilterPredicate::False,
            1 => flattened.into_iter().next().unwrap(),
            _ => FilterPredicate::Or(flattened),
        }
    }

    /// Create a NOT predicate
    pub fn not(predicate: FilterPredicate) -> Self {
        // Optimize: double negation and constant folding
        match predicate {
            FilterPredicate::True => FilterPredicate::False,
            FilterPredicate::False => FilterPredicate::True,
            FilterPredicate::Not(inner) => *inner,
            other => FilterPredicate::Not(Box::new(other)),
        }
    }

    /// Check if the predicate is always true
    pub fn is_always_true(&self) -> bool {
        matches!(self, FilterPredicate::True)
    }

    /// Check if the predicate is always false
    pub fn is_always_false(&self) -> bool {
        matches!(self, FilterPredicate::False)
    }
}

/// Filter evaluator that processes predicates on data blocks
pub struct FilterEvaluator {
    predicate: FilterPredicate,
    /// Cached column indices for faster access
    column_indices: Vec<(String, Option<usize>)>,
    /// Compiled regex patterns
    compiled_regexes: Vec<(String, Option<regex::Regex>)>,
    /// Compiled LIKE patterns
    compiled_likes: Vec<(String, Option<regex::Regex>)>,
}

impl FilterEvaluator {
    /// Create a new filter evaluator
    pub fn new(predicate: FilterPredicate) -> Self {
        let mut evaluator = Self {
            predicate,
            column_indices: Vec::new(),
            compiled_regexes: Vec::new(),
            compiled_likes: Vec::new(),
        };
        evaluator.compile_patterns();
        evaluator
    }

    /// Compile regex and LIKE patterns
    fn compile_patterns(&mut self) {
        self.compile_predicate(&self.predicate.clone());
    }

    fn compile_predicate(&mut self, predicate: &FilterPredicate) {
        match predicate {
            FilterPredicate::Regexp { column, pattern } => {
                let compiled = regex::Regex::new(pattern).ok();
                self.compiled_regexes.push((column.clone(), compiled));
            }
            FilterPredicate::Like { column, pattern, escape } => {
                let regex_pattern = like_to_regex(pattern, *escape);
                let compiled = regex::Regex::new(&regex_pattern).ok();
                self.compiled_likes.push((column.clone(), compiled));
            }
            FilterPredicate::And(children) | FilterPredicate::Or(children) => {
                for child in children {
                    self.compile_predicate(child);
                }
            }
            FilterPredicate::Not(inner) => {
                self.compile_predicate(inner);
            }
            _ => {}
        }
    }

    /// Evaluate the filter on a data block, returning indices of matching rows
    pub fn evaluate(&self, block: &DataBlock) -> FilterResult<Vec<usize>> {
        let num_rows = block.num_rows;
        if num_rows == 0 {
            return Ok(Vec::new());
        }

        // Create a match bitmap
        let mut matches = vec![true; num_rows];
        self.evaluate_predicate(&self.predicate, block, &mut matches)?;

        // Convert to indices
        Ok(matches
            .iter()
            .enumerate()
            .filter(|(_, &m)| m)
            .map(|(i, _)| i)
            .collect())
    }

    /// Check if a single row matches
    pub fn evaluate_row(&self, block: &DataBlock, row: usize) -> FilterResult<bool> {
        self.evaluate_predicate_row(&self.predicate, block, row)
    }

    /// Evaluate predicate on all rows, updating the match vector
    fn evaluate_predicate(
        &self,
        predicate: &FilterPredicate,
        block: &DataBlock,
        matches: &mut [bool],
    ) -> FilterResult<()> {
        match predicate {
            FilterPredicate::True => {}
            FilterPredicate::False => {
                for m in matches.iter_mut() {
                    *m = false;
                }
            }
            FilterPredicate::And(children) => {
                for child in children {
                    self.evaluate_predicate(child, block, matches)?;
                    // Short-circuit if all false
                    if matches.iter().all(|&m| !m) {
                        break;
                    }
                }
            }
            FilterPredicate::Or(children) => {
                let mut any_true = vec![false; matches.len()];
                for child in children {
                    let mut child_matches = matches.to_vec();
                    self.evaluate_predicate(child, block, &mut child_matches)?;
                    for (i, &m) in child_matches.iter().enumerate() {
                        if m {
                            any_true[i] = true;
                        }
                    }
                }
                for (i, &m) in any_true.iter().enumerate() {
                    matches[i] = matches[i] && m;
                }
            }
            FilterPredicate::Not(inner) => {
                let mut inner_matches = matches.to_vec();
                self.evaluate_predicate(inner, block, &mut inner_matches)?;
                for (i, &m) in inner_matches.iter().enumerate() {
                    matches[i] = matches[i] && !m;
                }
            }
            _ => {
                // Simple predicates: evaluate each row
                for i in 0..matches.len() {
                    if matches[i] {
                        matches[i] = self.evaluate_simple_predicate(predicate, block, i)?;
                    }
                }
            }
        }
        Ok(())
    }

    /// Evaluate a simple predicate on a single row
    fn evaluate_simple_predicate(
        &self,
        predicate: &FilterPredicate,
        block: &DataBlock,
        row: usize,
    ) -> FilterResult<bool> {
        match predicate {
            FilterPredicate::True => Ok(true),
            FilterPredicate::False => Ok(false),
            FilterPredicate::Eq { column, value } => self.eval_eq(block, row, column, value),
            FilterPredicate::NotEq { column, value } => {
                self.eval_eq(block, row, column, value).map(|r| !r)
            }
            FilterPredicate::Lt { column, value } => self.eval_cmp(block, row, column, value, |o| {
                matches!(o, Ordering::Less)
            }),
            FilterPredicate::Le { column, value } => self.eval_cmp(block, row, column, value, |o| {
                matches!(o, Ordering::Less | Ordering::Equal)
            }),
            FilterPredicate::Gt { column, value } => self.eval_cmp(block, row, column, value, |o| {
                matches!(o, Ordering::Greater)
            }),
            FilterPredicate::Ge { column, value } => self.eval_cmp(block, row, column, value, |o| {
                matches!(o, Ordering::Greater | Ordering::Equal)
            }),
            FilterPredicate::Range { column, bounds } => self.eval_range(block, row, column, bounds),
            FilterPredicate::In { column, values } => self.eval_in(block, row, column, values, false),
            FilterPredicate::NotIn { column, values } => self.eval_in(block, row, column, values, true),
            FilterPredicate::IsNull { column } => self.eval_is_null(block, row, column),
            FilterPredicate::IsNotNull { column } => {
                self.eval_is_null(block, row, column).map(|r| !r)
            }
            FilterPredicate::Like { column, pattern: _, escape: _ } => {
                self.eval_like(block, row, column)
            }
            FilterPredicate::Regexp { column, pattern: _ } => self.eval_regexp(block, row, column),
            FilterPredicate::And(children) => {
                for child in children {
                    if !self.evaluate_simple_predicate(child, block, row)? {
                        return Ok(false);
                    }
                }
                Ok(true)
            }
            FilterPredicate::Or(children) => {
                for child in children {
                    if self.evaluate_simple_predicate(child, block, row)? {
                        return Ok(true);
                    }
                }
                Ok(false)
            }
            FilterPredicate::Not(inner) => {
                self.evaluate_simple_predicate(inner, block, row).map(|r| !r)
            }
        }
    }

    /// Evaluate predicate on a single row (for row-by-row processing)
    fn evaluate_predicate_row(
        &self,
        predicate: &FilterPredicate,
        block: &DataBlock,
        row: usize,
    ) -> FilterResult<bool> {
        self.evaluate_simple_predicate(predicate, block, row)
    }

    /// Find column index by name
    fn find_column(&self, block: &DataBlock, column: &str) -> FilterResult<usize> {
        block
            .schema
            .columns
            .iter()
            .position(|c| c.name == column)
            .ok_or_else(|| FilterError::ColumnNotFound {
                name: column.to_string(),
            })
    }

    /// Get value from column at row
    fn get_value(&self, block: &DataBlock, row: usize, column: &str) -> FilterResult<ScalarValue> {
        let col_idx = self.find_column(block, column)?;
        let col = &block.columns[col_idx];

        // Check for null
        if let Some(null_bitmaps) = &block.null_bitmaps {
            if col_idx < null_bitmaps.len() && row < null_bitmaps[col_idx].len() {
                if null_bitmaps[col_idx][row] {
                    return Ok(ScalarValue::Null);
                }
            }
        }

        Ok(match col {
            ColumnData::Int(data) => ScalarValue::Int(data[row]),
            ColumnData::Long(data) => ScalarValue::Long(data[row]),
            ColumnData::Float(data) => ScalarValue::Float(data[row]),
            ColumnData::Double(data) => ScalarValue::Double(data[row]),
            ColumnData::String(data) => ScalarValue::String(data[row].clone()),
            ColumnData::Bytes(data) => ScalarValue::Bytes(data[row].clone()),
            ColumnData::Boolean(data) => ScalarValue::Boolean(data[row]),
            ColumnData::Nulls(data) => {
                if data[row] {
                    ScalarValue::Null
                } else {
                    ScalarValue::Boolean(false)
                }
            }
        })
    }

    /// Evaluate equality
    fn eval_eq(
        &self,
        block: &DataBlock,
        row: usize,
        column: &str,
        value: &ScalarValue,
    ) -> FilterResult<bool> {
        let col_value = self.get_value(block, row, column)?;

        // NULL comparison: NULL = anything is false (SQL semantics)
        if col_value.is_null() || value.is_null() {
            return Ok(false);
        }

        Ok(compare_values(&col_value, value) == Some(Ordering::Equal))
    }

    /// Evaluate comparison with ordering check
    fn eval_cmp<F>(
        &self,
        block: &DataBlock,
        row: usize,
        column: &str,
        value: &ScalarValue,
        check: F,
    ) -> FilterResult<bool>
    where
        F: Fn(Ordering) -> bool,
    {
        let col_value = self.get_value(block, row, column)?;

        // NULL comparison always false
        if col_value.is_null() || value.is_null() {
            return Ok(false);
        }

        match compare_values(&col_value, value) {
            Some(ord) => Ok(check(ord)),
            None => Ok(false), // Incompatible types
        }
    }

    /// Evaluate range predicate
    fn eval_range(
        &self,
        block: &DataBlock,
        row: usize,
        column: &str,
        bounds: &RangeBounds,
    ) -> FilterResult<bool> {
        let col_value = self.get_value(block, row, column)?;

        if col_value.is_null() {
            return Ok(false);
        }

        // Check lower bound
        if let Some(lower) = &bounds.lower {
            if lower.is_null() {
                return Ok(false);
            }
            match compare_values(&col_value, lower) {
                Some(Ordering::Less) => return Ok(false),
                Some(Ordering::Equal) if !bounds.lower_inclusive => return Ok(false),
                None => return Ok(false),
                _ => {}
            }
        }

        // Check upper bound
        if let Some(upper) = &bounds.upper {
            if upper.is_null() {
                return Ok(false);
            }
            match compare_values(&col_value, upper) {
                Some(Ordering::Greater) => return Ok(false),
                Some(Ordering::Equal) if !bounds.upper_inclusive => return Ok(false),
                None => return Ok(false),
                _ => {}
            }
        }

        Ok(true)
    }

    /// Evaluate IN / NOT IN predicate
    fn eval_in(
        &self,
        block: &DataBlock,
        row: usize,
        column: &str,
        values: &[ScalarValue],
        negate: bool,
    ) -> FilterResult<bool> {
        let col_value = self.get_value(block, row, column)?;

        if col_value.is_null() {
            return Ok(false);
        }

        let found = values.iter().any(|v| {
            if v.is_null() {
                false
            } else {
                compare_values(&col_value, v) == Some(Ordering::Equal)
            }
        });

        Ok(if negate { !found } else { found })
    }

    /// Evaluate IS NULL predicate
    fn eval_is_null(
        &self,
        block: &DataBlock,
        row: usize,
        column: &str,
    ) -> FilterResult<bool> {
        let col_idx = self.find_column(block, column)?;

        // Check null bitmap
        if let Some(null_bitmaps) = &block.null_bitmaps {
            if col_idx < null_bitmaps.len() && row < null_bitmaps[col_idx].len() {
                return Ok(null_bitmaps[col_idx][row]);
            }
        }

        // Check Nulls column type
        if let ColumnData::Nulls(data) = &block.columns[col_idx] {
            return Ok(data[row]);
        }

        Ok(false)
    }

    /// Evaluate LIKE predicate
    fn eval_like(&self, block: &DataBlock, row: usize, column: &str) -> FilterResult<bool> {
        let col_value = self.get_value(block, row, column)?;

        if col_value.is_null() {
            return Ok(false);
        }

        let string_val = match &col_value {
            ScalarValue::String(s) => s.as_str(),
            _ => return Ok(false),
        };

        // Find the compiled pattern for this column
        for (col_name, compiled) in &self.compiled_likes {
            if col_name == column {
                return Ok(compiled
                    .as_ref()
                    .map(|re| re.is_match(string_val))
                    .unwrap_or(false));
            }
        }

        Ok(false)
    }

    /// Evaluate REGEXP predicate
    fn eval_regexp(&self, block: &DataBlock, row: usize, column: &str) -> FilterResult<bool> {
        let col_value = self.get_value(block, row, column)?;

        if col_value.is_null() {
            return Ok(false);
        }

        let string_val = match &col_value {
            ScalarValue::String(s) => s.as_str(),
            _ => return Ok(false),
        };

        // Find the compiled pattern for this column
        for (col_name, compiled) in &self.compiled_regexes {
            if col_name == column {
                return Ok(compiled
                    .as_ref()
                    .map(|re| re.is_match(string_val))
                    .unwrap_or(false));
            }
        }

        Ok(false)
    }

    /// Get the inner predicate
    pub fn predicate(&self) -> &FilterPredicate {
        &self.predicate
    }
}

/// Compare two scalar values
fn compare_values(a: &ScalarValue, b: &ScalarValue) -> Option<Ordering> {
    match (a, b) {
        (ScalarValue::Null, _) | (_, ScalarValue::Null) => None,

        // Integer comparisons
        (ScalarValue::Int(a), ScalarValue::Int(b)) => Some(a.cmp(b)),
        (ScalarValue::Long(a), ScalarValue::Long(b)) => Some(a.cmp(b)),
        (ScalarValue::Int(a), ScalarValue::Long(b)) => Some((*a as i64).cmp(b)),
        (ScalarValue::Long(a), ScalarValue::Int(b)) => Some(a.cmp(&(*b as i64))),

        // Float comparisons
        (ScalarValue::Float(a), ScalarValue::Float(b)) => a.partial_cmp(b),
        (ScalarValue::Double(a), ScalarValue::Double(b)) => a.partial_cmp(b),
        (ScalarValue::Float(a), ScalarValue::Double(b)) => (*a as f64).partial_cmp(b),
        (ScalarValue::Double(a), ScalarValue::Float(b)) => a.partial_cmp(&(*b as f64)),

        // Int-float comparisons (promote to double)
        (ScalarValue::Int(a), ScalarValue::Float(b)) => (*a as f64).partial_cmp(&(*b as f64)),
        (ScalarValue::Int(a), ScalarValue::Double(b)) => (*a as f64).partial_cmp(b),
        (ScalarValue::Long(a), ScalarValue::Float(b)) => (*a as f64).partial_cmp(&(*b as f64)),
        (ScalarValue::Long(a), ScalarValue::Double(b)) => (*a as f64).partial_cmp(b),
        (ScalarValue::Float(a), ScalarValue::Int(b)) => (*a as f64).partial_cmp(&(*b as f64)),
        (ScalarValue::Double(a), ScalarValue::Int(b)) => a.partial_cmp(&(*b as f64)),
        (ScalarValue::Float(a), ScalarValue::Long(b)) => (*a as f64).partial_cmp(&(*b as f64)),
        (ScalarValue::Double(a), ScalarValue::Long(b)) => a.partial_cmp(&(*b as f64)),

        // String comparisons
        (ScalarValue::String(a), ScalarValue::String(b)) => Some(a.cmp(b)),

        // Bytes comparisons
        (ScalarValue::Bytes(a), ScalarValue::Bytes(b)) => Some(a.cmp(b)),

        // Boolean comparisons
        (ScalarValue::Boolean(a), ScalarValue::Boolean(b)) => Some(a.cmp(b)),

        // Incompatible types
        _ => None,
    }
}

/// Convert a SQL LIKE pattern to a regex pattern
fn like_to_regex(pattern: &str, escape: Option<char>) -> String {
    let mut regex = String::from("^");
    let escape_char = escape.unwrap_or('\\');
    let mut chars = pattern.chars().peekable();

    while let Some(c) = chars.next() {
        if c == escape_char {
            // Escape next character
            if let Some(next) = chars.next() {
                regex.push_str(&regex::escape(&next.to_string()));
            }
        } else if c == '%' {
            regex.push_str(".*");
        } else if c == '_' {
            regex.push('.');
        } else {
            regex.push_str(&regex::escape(&c.to_string()));
        }
    }

    regex.push('$');
    regex
}

/// Create a filter function from a predicate
pub fn make_filter_fn(
    predicate: FilterPredicate,
) -> Arc<dyn Fn(&DataBlock, usize) -> bool + Send + Sync> {
    let evaluator = FilterEvaluator::new(predicate);
    Arc::new(move |block, row| evaluator.evaluate_row(block, row).unwrap_or(false))
}

/// Create a batch filter function from a predicate
pub fn make_batch_filter_fn(
    predicate: FilterPredicate,
) -> Arc<dyn Fn(&DataBlock) -> Vec<usize> + Send + Sync> {
    let evaluator = FilterEvaluator::new(predicate);
    Arc::new(move |block| evaluator.evaluate(block).unwrap_or_default())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::block::{BlockId, BlockSchema, ColumnSchema};

    fn create_test_block() -> DataBlock {
        let schema = Arc::new(BlockSchema::new(vec![
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
            ColumnSchema {
                name: "score".to_string(),
                data_type: ColumnType::Double,
                nullable: false,
            },
            ColumnSchema {
                name: "active".to_string(),
                data_type: ColumnType::Boolean,
                nullable: false,
            },
        ]));

        let columns = vec![
            ColumnData::Int(vec![1, 2, 3, 4, 5]),
            ColumnData::String(vec![
                "Alice".to_string(),
                "Bob".to_string(),
                "Charlie".to_string(),
                "David".to_string(),
                "Eve".to_string(),
            ]),
            ColumnData::Double(vec![85.5, 92.0, 78.3, 88.9, 95.2]),
            ColumnData::Boolean(vec![true, false, true, true, false]),
        ];

        DataBlock {
            id: BlockId::new("test", 0, 0, 0),
            schema,
            columns,
            num_rows: 5,
            null_bitmaps: Some(vec![
                vec![false, false, false, false, false], // id
                vec![false, false, false, true, false],  // name - David is null
                vec![false, false, false, false, false], // score
                vec![false, false, false, false, false], // active
            ]),
            created_at: None,
        }
    }

    #[test]
    fn test_eq_predicate() {
        let block = create_test_block();
        let pred = FilterPredicate::eq("id", 3);
        let evaluator = FilterEvaluator::new(pred);
        let matches = evaluator.evaluate(&block).unwrap();
        assert_eq!(matches, vec![2]);
    }

    #[test]
    fn test_not_eq_predicate() {
        let block = create_test_block();
        let pred = FilterPredicate::not_eq("id", 3);
        let evaluator = FilterEvaluator::new(pred);
        let matches = evaluator.evaluate(&block).unwrap();
        assert_eq!(matches, vec![0, 1, 3, 4]);
    }

    #[test]
    fn test_lt_predicate() {
        let block = create_test_block();
        let pred = FilterPredicate::lt("id", 3);
        let evaluator = FilterEvaluator::new(pred);
        let matches = evaluator.evaluate(&block).unwrap();
        assert_eq!(matches, vec![0, 1]);
    }

    #[test]
    fn test_le_predicate() {
        let block = create_test_block();
        let pred = FilterPredicate::le("id", 3);
        let evaluator = FilterEvaluator::new(pred);
        let matches = evaluator.evaluate(&block).unwrap();
        assert_eq!(matches, vec![0, 1, 2]);
    }

    #[test]
    fn test_gt_predicate() {
        let block = create_test_block();
        let pred = FilterPredicate::gt("score", 90.0);
        let evaluator = FilterEvaluator::new(pred);
        let matches = evaluator.evaluate(&block).unwrap();
        assert_eq!(matches, vec![1, 4]); // Bob=92.0, Eve=95.2
    }

    #[test]
    fn test_ge_predicate() {
        let block = create_test_block();
        let pred = FilterPredicate::ge("score", 92.0);
        let evaluator = FilterEvaluator::new(pred);
        let matches = evaluator.evaluate(&block).unwrap();
        assert_eq!(matches, vec![1, 4]); // Bob=92.0, Eve=95.2
    }

    #[test]
    fn test_between_predicate() {
        let block = create_test_block();
        let pred = FilterPredicate::between("score", 80.0f64, 90.0f64);
        let evaluator = FilterEvaluator::new(pred);
        let matches = evaluator.evaluate(&block).unwrap();
        assert_eq!(matches, vec![0, 3]); // Alice=85.5, David=88.9
    }

    #[test]
    fn test_in_predicate() {
        let block = create_test_block();
        let pred = FilterPredicate::in_list("name", vec!["Alice", "Eve"]);
        let evaluator = FilterEvaluator::new(pred);
        let matches = evaluator.evaluate(&block).unwrap();
        assert_eq!(matches, vec![0, 4]);
    }

    #[test]
    fn test_not_in_predicate() {
        let block = create_test_block();
        let pred = FilterPredicate::not_in_list("id", vec![1, 5]);
        let evaluator = FilterEvaluator::new(pred);
        let matches = evaluator.evaluate(&block).unwrap();
        assert_eq!(matches, vec![1, 2, 3]); // id 2, 3, 4
    }

    #[test]
    fn test_is_null_predicate() {
        let block = create_test_block();
        let pred = FilterPredicate::is_null("name");
        let evaluator = FilterEvaluator::new(pred);
        let matches = evaluator.evaluate(&block).unwrap();
        assert_eq!(matches, vec![3]); // David's name is null
    }

    #[test]
    fn test_is_not_null_predicate() {
        let block = create_test_block();
        let pred = FilterPredicate::is_not_null("name");
        let evaluator = FilterEvaluator::new(pred);
        let matches = evaluator.evaluate(&block).unwrap();
        assert_eq!(matches, vec![0, 1, 2, 4]);
    }

    #[test]
    fn test_like_predicate() {
        let block = create_test_block();
        let pred = FilterPredicate::like("name", "A%");
        let evaluator = FilterEvaluator::new(pred);
        let matches = evaluator.evaluate(&block).unwrap();
        assert_eq!(matches, vec![0]); // Alice
    }

    #[test]
    fn test_like_underscore() {
        let block = create_test_block();
        let pred = FilterPredicate::like("name", "B__");
        let evaluator = FilterEvaluator::new(pred);
        let matches = evaluator.evaluate(&block).unwrap();
        assert_eq!(matches, vec![1]); // Bob
    }

    #[test]
    fn test_regexp_predicate() {
        let block = create_test_block();
        let pred = FilterPredicate::regexp("name", "^[A-E].*$");
        let evaluator = FilterEvaluator::new(pred);
        let matches = evaluator.evaluate(&block).unwrap();
        assert_eq!(matches, vec![0, 1, 2, 4]); // Alice, Bob, Charlie, Eve (not David - null)
    }

    #[test]
    fn test_and_predicate() {
        let block = create_test_block();
        let pred = FilterPredicate::and(vec![
            FilterPredicate::ge("score", 85.0),
            FilterPredicate::eq("active", true),
        ]);
        let evaluator = FilterEvaluator::new(pred);
        let matches = evaluator.evaluate(&block).unwrap();
        assert_eq!(matches, vec![0, 3]); // Alice (85.5, true), David (88.9, true)
    }

    #[test]
    fn test_or_predicate() {
        let block = create_test_block();
        let pred = FilterPredicate::or(vec![
            FilterPredicate::eq("id", 1),
            FilterPredicate::eq("id", 5),
        ]);
        let evaluator = FilterEvaluator::new(pred);
        let matches = evaluator.evaluate(&block).unwrap();
        assert_eq!(matches, vec![0, 4]);
    }

    #[test]
    fn test_not_predicate() {
        let block = create_test_block();
        let pred = FilterPredicate::not(FilterPredicate::eq("active", true));
        let evaluator = FilterEvaluator::new(pred);
        let matches = evaluator.evaluate(&block).unwrap();
        assert_eq!(matches, vec![1, 4]); // Bob, Eve (inactive)
    }

    #[test]
    fn test_complex_predicate() {
        let block = create_test_block();
        // (score > 80 AND active = true) OR id = 2
        let pred = FilterPredicate::or(vec![
            FilterPredicate::and(vec![
                FilterPredicate::gt("score", 80.0),
                FilterPredicate::eq("active", true),
            ]),
            FilterPredicate::eq("id", 2),
        ]);
        let evaluator = FilterEvaluator::new(pred);
        let matches = evaluator.evaluate(&block).unwrap();
        // Alice (85.5, true), Bob (id=2), David (88.9, true)
        assert_eq!(matches, vec![0, 1, 3]);
    }

    #[test]
    fn test_always_true() {
        let block = create_test_block();
        let pred = FilterPredicate::True;
        let evaluator = FilterEvaluator::new(pred);
        let matches = evaluator.evaluate(&block).unwrap();
        assert_eq!(matches, vec![0, 1, 2, 3, 4]);
    }

    #[test]
    fn test_always_false() {
        let block = create_test_block();
        let pred = FilterPredicate::False;
        let evaluator = FilterEvaluator::new(pred);
        let matches = evaluator.evaluate(&block).unwrap();
        assert!(matches.is_empty());
    }

    #[test]
    fn test_null_comparison_returns_false() {
        let block = create_test_block();
        // Comparing with null (David's name is null)
        let pred = FilterPredicate::eq("name", "David");
        let evaluator = FilterEvaluator::new(pred);
        let matches = evaluator.evaluate(&block).unwrap();
        // David's name is NULL, so equality fails
        assert!(!matches.contains(&3));
    }

    #[test]
    fn test_and_optimization() {
        // AND with True should be removed
        let pred = FilterPredicate::and(vec![FilterPredicate::True, FilterPredicate::eq("id", 1)]);
        assert!(matches!(pred, FilterPredicate::Eq { .. }));

        // AND with False should return False
        let pred = FilterPredicate::and(vec![FilterPredicate::False, FilterPredicate::eq("id", 1)]);
        assert!(matches!(pred, FilterPredicate::False));
    }

    #[test]
    fn test_or_optimization() {
        // OR with False should be removed
        let pred = FilterPredicate::or(vec![FilterPredicate::False, FilterPredicate::eq("id", 1)]);
        assert!(matches!(pred, FilterPredicate::Eq { .. }));

        // OR with True should return True
        let pred = FilterPredicate::or(vec![FilterPredicate::True, FilterPredicate::eq("id", 1)]);
        assert!(matches!(pred, FilterPredicate::True));
    }

    #[test]
    fn test_not_optimization() {
        // Double negation
        let pred = FilterPredicate::not(FilterPredicate::not(FilterPredicate::eq("id", 1)));
        assert!(matches!(pred, FilterPredicate::Eq { .. }));

        // NOT True = False
        let pred = FilterPredicate::not(FilterPredicate::True);
        assert!(matches!(pred, FilterPredicate::False));

        // NOT False = True
        let pred = FilterPredicate::not(FilterPredicate::False);
        assert!(matches!(pred, FilterPredicate::True));
    }

    #[test]
    fn test_like_to_regex() {
        assert_eq!(like_to_regex("hello", None), "^hello$");
        assert_eq!(like_to_regex("he%", None), "^he.*$");
        assert_eq!(like_to_regex("he_lo", None), "^he.lo$");
        assert_eq!(like_to_regex("h%_o", None), "^h.*.o$");
        assert_eq!(like_to_regex("h\\%llo", None), "^h%llo$"); // escaped %
    }

    #[test]
    fn test_make_filter_fn() {
        let block = create_test_block();
        let pred = FilterPredicate::gt("score", 90.0);
        let filter = make_filter_fn(pred);

        let matches: Vec<usize> = (0..block.num_rows)
            .filter(|&i| filter(&block, i))
            .collect();
        assert_eq!(matches, vec![1, 4]);
    }

    #[test]
    fn test_make_batch_filter_fn() {
        let block = create_test_block();
        let pred = FilterPredicate::le("id", 3);
        let filter = make_batch_filter_fn(pred);

        let matches = filter(&block);
        assert_eq!(matches, vec![0, 1, 2]);
    }

    #[test]
    fn test_empty_block() {
        let schema = Arc::new(BlockSchema::new(vec![ColumnSchema {
            name: "id".to_string(),
            data_type: ColumnType::Int,
            nullable: false,
        }]));

        let block = DataBlock {
            id: BlockId::new("test", 0, 0, 0),
            schema,
            columns: vec![ColumnData::Int(vec![])],
            num_rows: 0,
            null_bitmaps: None,
            created_at: None,
        };

        let pred = FilterPredicate::eq("id", 1);
        let evaluator = FilterEvaluator::new(pred);
        let matches = evaluator.evaluate(&block).unwrap();
        assert!(matches.is_empty());
    }

    #[test]
    fn test_type_coercion() {
        let block = create_test_block();

        // Compare int column with long value
        let pred = FilterPredicate::eq("id", ScalarValue::Long(3));
        let evaluator = FilterEvaluator::new(pred);
        let matches = evaluator.evaluate(&block).unwrap();
        assert_eq!(matches, vec![2]);

        // Compare double column with int value
        let pred = FilterPredicate::gt("score", ScalarValue::Int(90));
        let evaluator = FilterEvaluator::new(pred);
        let matches = evaluator.evaluate(&block).unwrap();
        assert_eq!(matches, vec![1, 4]);
    }

    #[test]
    fn test_column_not_found() {
        let block = create_test_block();
        let pred = FilterPredicate::eq("nonexistent", 1);
        let evaluator = FilterEvaluator::new(pred);
        let result = evaluator.evaluate(&block);
        assert!(result.is_err());
        match result {
            Err(FilterError::ColumnNotFound { name }) => {
                assert_eq!(name, "nonexistent");
            }
            _ => panic!("Expected ColumnNotFound error"),
        }
    }
}
