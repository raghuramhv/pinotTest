//! Filter operators
//!
//! Provides filter operator implementations for evaluating predicates
//! and producing DocIdSets.

use super::doc_id_set::*;
use roaring::RoaringBitmap;

/// Predicate types
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PredicateType {
    Eq,
    NotEq,
    Lt,
    Le,
    Gt,
    Ge,
    Between,
    In,
    NotIn,
    Regexp,
    Like,
    IsNull,
    IsNotNull,
}

/// Trait for predicate evaluators
pub trait PredicateEvaluator: Send + Sync {
    /// Returns the predicate type
    fn predicate_type(&self) -> PredicateType;

    /// Returns true if this predicate is always true
    fn is_always_true(&self) -> bool {
        false
    }

    /// Returns true if this predicate is always false
    fn is_always_false(&self) -> bool {
        false
    }

    /// Evaluates the predicate for a single value (dictionary ID)
    fn apply_sv(&self, dict_id: i32) -> bool;

    /// Evaluates the predicate for multiple values (multi-value column)
    fn apply_mv(&self, dict_ids: &[i32]) -> bool {
        dict_ids.iter().any(|&id| self.apply_sv(id))
    }

    /// Returns matching dictionary IDs
    fn matching_dict_ids(&self) -> Option<&[i32]> {
        None
    }

    /// Returns non-matching dictionary IDs
    fn non_matching_dict_ids(&self) -> Option<&[i32]> {
        None
    }

    /// Returns true if this is an exclusive predicate (NOT IN, etc.)
    fn is_exclusive(&self) -> bool {
        false
    }
}

/// Equality predicate evaluator
pub struct EqPredicateEvaluator {
    target_dict_id: i32,
}

impl EqPredicateEvaluator {
    pub fn new(target_dict_id: i32) -> Self {
        Self { target_dict_id }
    }
}

impl PredicateEvaluator for EqPredicateEvaluator {
    fn predicate_type(&self) -> PredicateType {
        PredicateType::Eq
    }

    fn apply_sv(&self, dict_id: i32) -> bool {
        dict_id == self.target_dict_id
    }
}

/// Not-equal predicate evaluator
pub struct NotEqPredicateEvaluator {
    target_dict_id: i32,
}

impl NotEqPredicateEvaluator {
    pub fn new(target_dict_id: i32) -> Self {
        Self { target_dict_id }
    }
}

impl PredicateEvaluator for NotEqPredicateEvaluator {
    fn predicate_type(&self) -> PredicateType {
        PredicateType::NotEq
    }

    fn apply_sv(&self, dict_id: i32) -> bool {
        dict_id != self.target_dict_id
    }

    fn is_exclusive(&self) -> bool {
        true
    }
}

/// IN predicate evaluator
pub struct InPredicateEvaluator {
    matching_ids: Vec<i32>,
    matching_set: std::collections::HashSet<i32>,
}

impl InPredicateEvaluator {
    pub fn new(matching_ids: Vec<i32>) -> Self {
        let matching_set = matching_ids.iter().copied().collect();
        Self {
            matching_ids,
            matching_set,
        }
    }
}

impl PredicateEvaluator for InPredicateEvaluator {
    fn predicate_type(&self) -> PredicateType {
        PredicateType::In
    }

    fn apply_sv(&self, dict_id: i32) -> bool {
        self.matching_set.contains(&dict_id)
    }

    fn matching_dict_ids(&self) -> Option<&[i32]> {
        Some(&self.matching_ids)
    }
}

/// NOT IN predicate evaluator
pub struct NotInPredicateEvaluator {
    excluded_ids: Vec<i32>,
    excluded_set: std::collections::HashSet<i32>,
}

impl NotInPredicateEvaluator {
    pub fn new(excluded_ids: Vec<i32>) -> Self {
        let excluded_set = excluded_ids.iter().copied().collect();
        Self {
            excluded_ids,
            excluded_set,
        }
    }
}

impl PredicateEvaluator for NotInPredicateEvaluator {
    fn predicate_type(&self) -> PredicateType {
        PredicateType::NotIn
    }

    fn apply_sv(&self, dict_id: i32) -> bool {
        !self.excluded_set.contains(&dict_id)
    }

    fn non_matching_dict_ids(&self) -> Option<&[i32]> {
        Some(&self.excluded_ids)
    }

    fn is_exclusive(&self) -> bool {
        true
    }
}

/// Range predicate evaluator (for sorted dictionaries)
pub struct RangePredicateEvaluator {
    min_dict_id: i32,      // inclusive
    max_dict_id: i32,      // exclusive
    include_min: bool,
    include_max: bool,
}

impl RangePredicateEvaluator {
    pub fn new(min_dict_id: i32, max_dict_id: i32, include_min: bool, include_max: bool) -> Self {
        Self {
            min_dict_id,
            max_dict_id,
            include_min,
            include_max,
        }
    }

    pub fn less_than(max_dict_id: i32) -> Self {
        Self::new(i32::MIN, max_dict_id, true, false)
    }

    pub fn less_than_or_equal(max_dict_id: i32) -> Self {
        Self::new(i32::MIN, max_dict_id, true, true)
    }

    pub fn greater_than(min_dict_id: i32) -> Self {
        Self::new(min_dict_id, i32::MAX, false, true)
    }

    pub fn greater_than_or_equal(min_dict_id: i32) -> Self {
        Self::new(min_dict_id, i32::MAX, true, true)
    }

    pub fn between(min_dict_id: i32, max_dict_id: i32) -> Self {
        Self::new(min_dict_id, max_dict_id, true, true)
    }
}

impl PredicateEvaluator for RangePredicateEvaluator {
    fn predicate_type(&self) -> PredicateType {
        PredicateType::Between
    }

    fn apply_sv(&self, dict_id: i32) -> bool {
        let lower_ok = if self.include_min {
            dict_id >= self.min_dict_id
        } else {
            dict_id > self.min_dict_id
        };

        let upper_ok = if self.include_max {
            dict_id <= self.max_dict_id
        } else {
            dict_id < self.max_dict_id
        };

        lower_ok && upper_ok
    }
}

/// Filter operator trait
pub trait FilterOperator: Send + Sync {
    /// Returns the DocIdSet of matching documents
    fn get_matching_doc_ids(&self) -> Box<dyn DocIdSet>;

    /// Returns the number of documents
    fn num_docs(&self) -> i32;

    /// Returns true if this operator can produce bitmaps directly
    fn can_produce_bitmaps(&self) -> bool {
        false
    }

    /// Returns the bitmap directly (if can_produce_bitmaps is true)
    fn get_bitmap(&self) -> Option<RoaringBitmap> {
        None
    }

    /// Returns true if count can be optimized
    fn can_optimize_count(&self) -> bool {
        false
    }

    /// Returns the count of matching documents (optimized)
    fn get_matching_count(&self) -> Option<u64> {
        None
    }
}

/// Bitmap-based filter operator
pub struct BitmapFilterOperator {
    bitmap: RoaringBitmap,
    num_docs: i32,
    exclusive: bool,
}

impl BitmapFilterOperator {
    pub fn new(bitmap: RoaringBitmap, num_docs: i32, exclusive: bool) -> Self {
        Self {
            bitmap,
            num_docs,
            exclusive,
        }
    }
}

impl FilterOperator for BitmapFilterOperator {
    fn get_matching_doc_ids(&self) -> Box<dyn DocIdSet> {
        if self.exclusive {
            let flipped = BitmapDocIdSet::new(self.bitmap.clone(), self.num_docs).flip();
            Box::new(flipped)
        } else {
            Box::new(BitmapDocIdSet::new(self.bitmap.clone(), self.num_docs))
        }
    }

    fn num_docs(&self) -> i32 {
        self.num_docs
    }

    fn can_produce_bitmaps(&self) -> bool {
        true
    }

    fn get_bitmap(&self) -> Option<RoaringBitmap> {
        if self.exclusive {
            let mut flipped = RoaringBitmap::new();
            flipped.insert_range(0..self.num_docs as u32);
            flipped -= &self.bitmap;
            Some(flipped)
        } else {
            Some(self.bitmap.clone())
        }
    }

    fn can_optimize_count(&self) -> bool {
        true
    }

    fn get_matching_count(&self) -> Option<u64> {
        if self.exclusive {
            Some(self.num_docs as u64 - self.bitmap.len())
        } else {
            Some(self.bitmap.len())
        }
    }
}

/// Match-all filter operator
pub struct MatchAllFilterOperator {
    num_docs: i32,
}

impl MatchAllFilterOperator {
    pub fn new(num_docs: i32) -> Self {
        Self { num_docs }
    }
}

impl FilterOperator for MatchAllFilterOperator {
    fn get_matching_doc_ids(&self) -> Box<dyn DocIdSet> {
        Box::new(MatchAllDocIdSet::new(self.num_docs))
    }

    fn num_docs(&self) -> i32 {
        self.num_docs
    }

    fn can_optimize_count(&self) -> bool {
        true
    }

    fn get_matching_count(&self) -> Option<u64> {
        Some(self.num_docs as u64)
    }
}

/// Empty filter operator
pub struct EmptyFilterOperator {
    num_docs: i32,
}

impl EmptyFilterOperator {
    pub fn new(num_docs: i32) -> Self {
        Self { num_docs }
    }
}

impl FilterOperator for EmptyFilterOperator {
    fn get_matching_doc_ids(&self) -> Box<dyn DocIdSet> {
        Box::new(EmptyDocIdSet)
    }

    fn num_docs(&self) -> i32 {
        self.num_docs
    }

    fn can_optimize_count(&self) -> bool {
        true
    }

    fn get_matching_count(&self) -> Option<u64> {
        Some(0)
    }
}

/// AND filter operator
pub struct AndFilterOperator {
    children: Vec<Box<dyn FilterOperator>>,
    num_docs: i32,
}

impl AndFilterOperator {
    pub fn new(children: Vec<Box<dyn FilterOperator>>, num_docs: i32) -> Self {
        Self { children, num_docs }
    }
}

impl FilterOperator for AndFilterOperator {
    fn get_matching_doc_ids(&self) -> Box<dyn DocIdSet> {
        let doc_id_sets: Vec<Box<dyn DocIdSet>> = self
            .children
            .iter()
            .map(|op| op.get_matching_doc_ids())
            .collect();

        AndDocIdSet::optimized(doc_id_sets, self.num_docs)
    }

    fn num_docs(&self) -> i32 {
        self.num_docs
    }

    fn can_optimize_count(&self) -> bool {
        self.children.iter().all(|c| c.can_produce_bitmaps())
    }

    fn get_matching_count(&self) -> Option<u64> {
        if !self.can_optimize_count() {
            return None;
        }

        let bitmaps: Vec<RoaringBitmap> = self
            .children
            .iter()
            .filter_map(|c| c.get_bitmap())
            .collect();

        if bitmaps.is_empty() {
            return Some(self.num_docs as u64);
        }

        let refs: Vec<&RoaringBitmap> = bitmaps.iter().collect();
        let result = bitmap_ops::and_bitmaps(&refs);
        Some(result.len())
    }
}

/// OR filter operator
pub struct OrFilterOperator {
    children: Vec<Box<dyn FilterOperator>>,
    num_docs: i32,
}

impl OrFilterOperator {
    pub fn new(children: Vec<Box<dyn FilterOperator>>, num_docs: i32) -> Self {
        Self { children, num_docs }
    }
}

impl FilterOperator for OrFilterOperator {
    fn get_matching_doc_ids(&self) -> Box<dyn DocIdSet> {
        let doc_id_sets: Vec<Box<dyn DocIdSet>> = self
            .children
            .iter()
            .map(|op| op.get_matching_doc_ids())
            .collect();

        OrDocIdSet::optimized(doc_id_sets, self.num_docs)
    }

    fn num_docs(&self) -> i32 {
        self.num_docs
    }

    fn can_optimize_count(&self) -> bool {
        self.children.iter().all(|c| c.can_produce_bitmaps())
    }

    fn get_matching_count(&self) -> Option<u64> {
        if !self.can_optimize_count() {
            return None;
        }

        let bitmaps: Vec<RoaringBitmap> = self
            .children
            .iter()
            .filter_map(|c| c.get_bitmap())
            .collect();

        if bitmaps.is_empty() {
            return Some(0);
        }

        let refs: Vec<&RoaringBitmap> = bitmaps.iter().collect();
        let result = bitmap_ops::or_bitmaps(&refs);
        Some(result.len())
    }
}

/// NOT filter operator
pub struct NotFilterOperator {
    child: Box<dyn FilterOperator>,
    num_docs: i32,
}

impl NotFilterOperator {
    pub fn new(child: Box<dyn FilterOperator>, num_docs: i32) -> Self {
        Self { child, num_docs }
    }
}

impl FilterOperator for NotFilterOperator {
    fn get_matching_doc_ids(&self) -> Box<dyn DocIdSet> {
        let child_set = self.child.get_matching_doc_ids();
        Box::new(NotDocIdSet::new(child_set, self.num_docs))
    }

    fn num_docs(&self) -> i32 {
        self.num_docs
    }

    fn can_optimize_count(&self) -> bool {
        self.child.can_optimize_count()
    }

    fn get_matching_count(&self) -> Option<u64> {
        self.child
            .get_matching_count()
            .map(|c| self.num_docs as u64 - c)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::EOF;

    #[test]
    fn test_eq_predicate() {
        let pred = EqPredicateEvaluator::new(5);

        assert!(pred.apply_sv(5));
        assert!(!pred.apply_sv(4));
        assert!(!pred.apply_sv(6));
    }

    #[test]
    fn test_not_eq_predicate() {
        let pred = NotEqPredicateEvaluator::new(5);

        assert!(!pred.apply_sv(5));
        assert!(pred.apply_sv(4));
        assert!(pred.apply_sv(6));
        assert!(pred.is_exclusive());
    }

    #[test]
    fn test_in_predicate() {
        let pred = InPredicateEvaluator::new(vec![1, 3, 5, 7]);

        assert!(pred.apply_sv(1));
        assert!(!pred.apply_sv(2));
        assert!(pred.apply_sv(3));
        assert!(pred.apply_sv(5));
        assert!(!pred.apply_sv(6));
    }

    #[test]
    fn test_not_in_predicate() {
        let pred = NotInPredicateEvaluator::new(vec![1, 3, 5]);

        assert!(!pred.apply_sv(1));
        assert!(pred.apply_sv(2));
        assert!(!pred.apply_sv(3));
        assert!(pred.apply_sv(4));
        assert!(pred.is_exclusive());
    }

    #[test]
    fn test_range_predicate() {
        let pred = RangePredicateEvaluator::between(3, 7);

        assert!(!pred.apply_sv(2));
        assert!(pred.apply_sv(3));
        assert!(pred.apply_sv(5));
        assert!(pred.apply_sv(7));
        assert!(!pred.apply_sv(8));
    }

    #[test]
    fn test_range_predicate_exclusive() {
        let pred = RangePredicateEvaluator::new(3, 7, false, false);

        assert!(!pred.apply_sv(3));
        assert!(pred.apply_sv(4));
        assert!(pred.apply_sv(6));
        assert!(!pred.apply_sv(7));
    }

    #[test]
    fn test_range_predicate_helpers() {
        let lt = RangePredicateEvaluator::less_than(5);
        assert!(lt.apply_sv(4));
        assert!(!lt.apply_sv(5));

        let le = RangePredicateEvaluator::less_than_or_equal(5);
        assert!(le.apply_sv(5));
        assert!(!le.apply_sv(6));

        let gt = RangePredicateEvaluator::greater_than(5);
        assert!(!gt.apply_sv(5));
        assert!(gt.apply_sv(6));

        let ge = RangePredicateEvaluator::greater_than_or_equal(5);
        assert!(ge.apply_sv(5));
        assert!(!ge.apply_sv(4));
    }

    #[test]
    fn test_bitmap_filter_operator() {
        let mut bitmap = RoaringBitmap::new();
        bitmap.insert(1);
        bitmap.insert(3);
        bitmap.insert(5);

        let op = BitmapFilterOperator::new(bitmap, 10, false);

        assert!(op.can_produce_bitmaps());
        assert!(op.can_optimize_count());
        assert_eq!(op.get_matching_count(), Some(3));

        let doc_ids = op.get_matching_doc_ids();
        let mut iter = doc_ids.iterator();
        assert_eq!(iter.next(), 1);
        assert_eq!(iter.next(), 3);
        assert_eq!(iter.next(), 5);
        assert_eq!(iter.next(), EOF);
    }

    #[test]
    fn test_bitmap_filter_operator_exclusive() {
        let mut bitmap = RoaringBitmap::new();
        bitmap.insert(1);
        bitmap.insert(3);

        let op = BitmapFilterOperator::new(bitmap, 5, true);

        assert_eq!(op.get_matching_count(), Some(3)); // 5 - 2 = 3

        let doc_ids = op.get_matching_doc_ids();
        let mut iter = doc_ids.iterator();
        assert_eq!(iter.next(), 0);
        assert_eq!(iter.next(), 2);
        assert_eq!(iter.next(), 4);
        assert_eq!(iter.next(), EOF);
    }

    #[test]
    fn test_and_filter_operator() {
        let mut bitmap1 = RoaringBitmap::new();
        bitmap1.insert_range(0..5);

        let mut bitmap2 = RoaringBitmap::new();
        bitmap2.insert_range(3..8);

        let op1: Box<dyn FilterOperator> =
            Box::new(BitmapFilterOperator::new(bitmap1, 10, false));
        let op2: Box<dyn FilterOperator> =
            Box::new(BitmapFilterOperator::new(bitmap2, 10, false));

        let and_op = AndFilterOperator::new(vec![op1, op2], 10);

        assert!(and_op.can_optimize_count());
        assert_eq!(and_op.get_matching_count(), Some(2)); // 3, 4

        let doc_ids = and_op.get_matching_doc_ids();
        let mut iter = doc_ids.iterator();
        assert_eq!(iter.next(), 3);
        assert_eq!(iter.next(), 4);
        assert_eq!(iter.next(), EOF);
    }

    #[test]
    fn test_or_filter_operator() {
        let mut bitmap1 = RoaringBitmap::new();
        bitmap1.insert(1);
        bitmap1.insert(2);

        let mut bitmap2 = RoaringBitmap::new();
        bitmap2.insert(3);
        bitmap2.insert(4);

        let op1: Box<dyn FilterOperator> =
            Box::new(BitmapFilterOperator::new(bitmap1, 10, false));
        let op2: Box<dyn FilterOperator> =
            Box::new(BitmapFilterOperator::new(bitmap2, 10, false));

        let or_op = OrFilterOperator::new(vec![op1, op2], 10);

        assert!(or_op.can_optimize_count());
        assert_eq!(or_op.get_matching_count(), Some(4));
    }

    #[test]
    fn test_not_filter_operator() {
        let mut bitmap = RoaringBitmap::new();
        bitmap.insert(1);
        bitmap.insert(3);

        let child: Box<dyn FilterOperator> =
            Box::new(BitmapFilterOperator::new(bitmap, 5, false));
        let not_op = NotFilterOperator::new(child, 5);

        assert!(not_op.can_optimize_count());
        assert_eq!(not_op.get_matching_count(), Some(3)); // 5 - 2

        let doc_ids = not_op.get_matching_doc_ids();
        let mut iter = doc_ids.iterator();
        assert_eq!(iter.next(), 0);
        assert_eq!(iter.next(), 2);
        assert_eq!(iter.next(), 4);
        assert_eq!(iter.next(), EOF);
    }

    #[test]
    fn test_match_all_filter_operator() {
        let op = MatchAllFilterOperator::new(5);

        assert!(op.can_optimize_count());
        assert_eq!(op.get_matching_count(), Some(5));

        let doc_ids = op.get_matching_doc_ids();
        assert!(doc_ids.is_match_all());
    }

    #[test]
    fn test_empty_filter_operator() {
        let op = EmptyFilterOperator::new(5);

        assert!(op.can_optimize_count());
        assert_eq!(op.get_matching_count(), Some(0));

        let doc_ids = op.get_matching_doc_ids();
        assert!(doc_ids.is_empty());
    }
}
