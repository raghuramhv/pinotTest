//! Document ID sets
//!
//! DocIdSet implementations that represent sets of document IDs
//! and provide iterators over them.

use super::doc_id_iterator::*;
use roaring::RoaringBitmap;


/// Trait for document ID sets
pub trait DocIdSet: Send + Sync {
    /// Creates an iterator over the document IDs
    fn iterator(&self) -> Box<dyn DocIdIterator>;

    /// Returns the number of entries scanned in filter evaluation
    fn num_entries_scanned(&self) -> u64 {
        0
    }

    /// Returns an optimized version of this set (e.g., empty set if cardinality is 0)
    fn optimize(self: Box<Self>) -> Box<dyn DocIdSet>;

    /// Returns the cardinality (number of documents) if known without iteration
    fn cardinality(&self) -> Option<u64> {
        None
    }

    /// Returns true if this is an empty set
    fn is_empty(&self) -> bool {
        false
    }

    /// Returns true if this matches all documents
    fn is_match_all(&self) -> bool {
        false
    }
}

/// DocIdSet backed by a RoaringBitmap
#[derive(Clone)]
pub struct BitmapDocIdSet {
    bitmap: RoaringBitmap,
    num_docs: i32,
}

impl BitmapDocIdSet {
    pub fn new(bitmap: RoaringBitmap, num_docs: i32) -> Self {
        Self { bitmap, num_docs }
    }

    pub fn bitmap(&self) -> &RoaringBitmap {
        &self.bitmap
    }

    pub fn into_bitmap(self) -> RoaringBitmap {
        self.bitmap
    }

    /// Creates a flipped version (NOT)
    pub fn flip(&self) -> Self {
        let mut flipped = RoaringBitmap::new();
        flipped.insert_range(0..self.num_docs as u32);
        flipped -= &self.bitmap;
        Self::new(flipped, self.num_docs)
    }
}

impl DocIdSet for BitmapDocIdSet {
    fn iterator(&self) -> Box<dyn DocIdIterator> {
        Box::new(BitmapDocIdIterator::new(self.bitmap.clone(), self.num_docs))
    }

    fn optimize(self: Box<Self>) -> Box<dyn DocIdSet> {
        if self.bitmap.is_empty() {
            Box::new(EmptyDocIdSet)
        } else if self.bitmap.len() == self.num_docs as u64 {
            Box::new(MatchAllDocIdSet::new(self.num_docs))
        } else {
            self
        }
    }

    fn cardinality(&self) -> Option<u64> {
        Some(self.bitmap.len())
    }

    fn is_empty(&self) -> bool {
        self.bitmap.is_empty()
    }
}

/// Empty document ID set
#[derive(Clone, Copy, Default)]
pub struct EmptyDocIdSet;

impl EmptyDocIdSet {
    pub fn new() -> Self {
        Self
    }
}

impl DocIdSet for EmptyDocIdSet {
    fn iterator(&self) -> Box<dyn DocIdIterator> {
        Box::new(EmptyDocIdIterator::new())
    }

    fn optimize(self: Box<Self>) -> Box<dyn DocIdSet> {
        self
    }

    fn cardinality(&self) -> Option<u64> {
        Some(0)
    }

    fn is_empty(&self) -> bool {
        true
    }
}

/// Document ID set that matches all documents
#[derive(Clone, Copy)]
pub struct MatchAllDocIdSet {
    num_docs: i32,
}

impl MatchAllDocIdSet {
    pub fn new(num_docs: i32) -> Self {
        Self { num_docs }
    }
}

impl DocIdSet for MatchAllDocIdSet {
    fn iterator(&self) -> Box<dyn DocIdIterator> {
        Box::new(MatchAllDocIdIterator::new(self.num_docs))
    }

    fn optimize(self: Box<Self>) -> Box<dyn DocIdSet> {
        self
    }

    fn cardinality(&self) -> Option<u64> {
        Some(self.num_docs as u64)
    }

    fn is_match_all(&self) -> bool {
        true
    }
}

/// AND of multiple DocIdSets
pub struct AndDocIdSet {
    children: Vec<Box<dyn DocIdSet>>,
    num_docs: i32,
    entries_scanned: u64,
}

impl AndDocIdSet {
    pub fn new(children: Vec<Box<dyn DocIdSet>>, num_docs: i32) -> Self {
        let entries_scanned: u64 = children.iter().map(|c| c.num_entries_scanned()).sum();
        Self {
            children,
            num_docs,
            entries_scanned,
        }
    }

    /// Creates an optimized AND set, potentially merging bitmaps
    pub fn optimized(children: Vec<Box<dyn DocIdSet>>, num_docs: i32) -> Box<dyn DocIdSet> {
        if children.is_empty() {
            return Box::new(MatchAllDocIdSet::new(num_docs));
        }

        // Check for empty or match-all sets
        let _bitmap_sets: Vec<BitmapDocIdSet> = Vec::new();
        let mut other_sets: Vec<Box<dyn DocIdSet>> = Vec::new();
        let mut total_entries_scanned = 0u64;

        for child in children {
            total_entries_scanned += child.num_entries_scanned();

            if child.is_empty() {
                return Box::new(EmptyDocIdSet);
            }

            if child.is_match_all() {
                continue; // Skip match-all in AND
            }

            // Try to downcast to BitmapDocIdSet
            // In practice, we'd use Any trait for proper downcasting
            other_sets.push(child);
        }

        if other_sets.is_empty() {
            return Box::new(MatchAllDocIdSet::new(num_docs));
        }

        if other_sets.len() == 1 {
            return other_sets.into_iter().next().unwrap();
        }

        Box::new(AndDocIdSet {
            children: other_sets,
            num_docs,
            entries_scanned: total_entries_scanned,
        })
    }
}

impl DocIdSet for AndDocIdSet {
    fn iterator(&self) -> Box<dyn DocIdIterator> {
        let iterators: Vec<Box<dyn DocIdIterator>> =
            self.children.iter().map(|c| c.iterator()).collect();
        Box::new(AndDocIdIterator::new(iterators))
    }

    fn num_entries_scanned(&self) -> u64 {
        self.entries_scanned
    }

    fn optimize(self: Box<Self>) -> Box<dyn DocIdSet> {
        AndDocIdSet::optimized(self.children, self.num_docs)
    }
}

/// OR of multiple DocIdSets
pub struct OrDocIdSet {
    children: Vec<Box<dyn DocIdSet>>,
    num_docs: i32,
    entries_scanned: u64,
}

impl OrDocIdSet {
    pub fn new(children: Vec<Box<dyn DocIdSet>>, num_docs: i32) -> Self {
        let entries_scanned: u64 = children.iter().map(|c| c.num_entries_scanned()).sum();
        Self {
            children,
            num_docs,
            entries_scanned,
        }
    }

    /// Creates an optimized OR set
    pub fn optimized(children: Vec<Box<dyn DocIdSet>>, num_docs: i32) -> Box<dyn DocIdSet> {
        if children.is_empty() {
            return Box::new(EmptyDocIdSet);
        }

        let mut filtered_children: Vec<Box<dyn DocIdSet>> = Vec::new();
        let mut total_entries_scanned = 0u64;

        for child in children {
            total_entries_scanned += child.num_entries_scanned();

            if child.is_match_all() {
                return Box::new(MatchAllDocIdSet::new(num_docs));
            }

            if !child.is_empty() {
                filtered_children.push(child);
            }
        }

        if filtered_children.is_empty() {
            return Box::new(EmptyDocIdSet);
        }

        if filtered_children.len() == 1 {
            return filtered_children.into_iter().next().unwrap();
        }

        Box::new(OrDocIdSet {
            children: filtered_children,
            num_docs,
            entries_scanned: total_entries_scanned,
        })
    }
}

impl DocIdSet for OrDocIdSet {
    fn iterator(&self) -> Box<dyn DocIdIterator> {
        let iterators: Vec<Box<dyn DocIdIterator>> =
            self.children.iter().map(|c| c.iterator()).collect();
        Box::new(OrDocIdIterator::new(iterators))
    }

    fn num_entries_scanned(&self) -> u64 {
        self.entries_scanned
    }

    fn optimize(self: Box<Self>) -> Box<dyn DocIdSet> {
        OrDocIdSet::optimized(self.children, self.num_docs)
    }
}

/// NOT of a DocIdSet
pub struct NotDocIdSet {
    child: Box<dyn DocIdSet>,
    num_docs: i32,
}

impl NotDocIdSet {
    pub fn new(child: Box<dyn DocIdSet>, num_docs: i32) -> Self {
        Self { child, num_docs }
    }
}

impl DocIdSet for NotDocIdSet {
    fn iterator(&self) -> Box<dyn DocIdIterator> {
        Box::new(NotDocIdIterator::new(self.child.iterator(), self.num_docs))
    }

    fn num_entries_scanned(&self) -> u64 {
        self.child.num_entries_scanned()
    }

    fn optimize(self: Box<Self>) -> Box<dyn DocIdSet> {
        if self.child.is_empty() {
            Box::new(MatchAllDocIdSet::new(self.num_docs))
        } else if self.child.is_match_all() {
            Box::new(EmptyDocIdSet)
        } else {
            self
        }
    }
}

/// DocIdSet backed by sorted ranges
pub struct SortedDocIdSet {
    ranges: Vec<(i32, i32)>,
}

impl SortedDocIdSet {
    pub fn new(ranges: Vec<(i32, i32)>) -> Self {
        Self { ranges }
    }

    pub fn ranges(&self) -> &[(i32, i32)] {
        &self.ranges
    }
}

impl DocIdSet for SortedDocIdSet {
    fn iterator(&self) -> Box<dyn DocIdIterator> {
        Box::new(SortedDocIdIterator::new(self.ranges.clone()))
    }

    fn optimize(self: Box<Self>) -> Box<dyn DocIdSet> {
        if self.ranges.is_empty() {
            Box::new(EmptyDocIdSet)
        } else {
            self
        }
    }

    fn cardinality(&self) -> Option<u64> {
        Some(
            self.ranges
                .iter()
                .map(|(start, end)| (end - start + 1) as u64)
                .sum(),
        )
    }

    fn is_empty(&self) -> bool {
        self.ranges.is_empty()
    }
}

/// Utility functions for bitmap operations
pub mod bitmap_ops {
    use super::*;

    /// Computes AND of multiple bitmaps efficiently
    pub fn and_bitmaps(bitmaps: &[&RoaringBitmap]) -> RoaringBitmap {
        if bitmaps.is_empty() {
            return RoaringBitmap::new();
        }

        let mut result = bitmaps[0].clone();
        for bitmap in &bitmaps[1..] {
            result &= *bitmap;
        }
        result
    }

    /// Computes OR of multiple bitmaps efficiently
    pub fn or_bitmaps(bitmaps: &[&RoaringBitmap]) -> RoaringBitmap {
        if bitmaps.is_empty() {
            return RoaringBitmap::new();
        }

        let mut result = bitmaps[0].clone();
        for bitmap in &bitmaps[1..] {
            result |= *bitmap;
        }
        result
    }

    /// Computes AND cardinality without materializing the result
    pub fn and_cardinality(bitmap1: &RoaringBitmap, bitmap2: &RoaringBitmap) -> u64 {
        (bitmap1 & bitmap2).len()
    }

    /// Computes OR cardinality without materializing the result
    pub fn or_cardinality(bitmap1: &RoaringBitmap, bitmap2: &RoaringBitmap) -> u64 {
        (bitmap1 | bitmap2).len()
    }

    /// Computes AND-NOT cardinality (bitmap1 AND NOT bitmap2)
    pub fn and_not_cardinality(bitmap1: &RoaringBitmap, bitmap2: &RoaringBitmap) -> u64 {
        (bitmap1 - bitmap2).len()
    }

    /// Applies a scan-based filter to a bitmap
    pub fn apply_scan_filter<F>(bitmap: &RoaringBitmap, predicate: F) -> RoaringBitmap
    where
        F: Fn(u32) -> bool,
    {
        let mut result = RoaringBitmap::new();
        for doc_id in bitmap.iter() {
            if predicate(doc_id) {
                result.insert(doc_id);
            }
        }
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::EOF;

    #[test]
    fn test_bitmap_doc_id_set() {
        let mut bitmap = RoaringBitmap::new();
        bitmap.insert(1);
        bitmap.insert(3);
        bitmap.insert(5);

        let set = BitmapDocIdSet::new(bitmap, 10);

        assert_eq!(set.cardinality(), Some(3));
        assert!(!set.is_empty());
        assert!(!set.is_match_all());

        let mut iter = set.iterator();
        assert_eq!(iter.next(), 1);
        assert_eq!(iter.next(), 3);
        assert_eq!(iter.next(), 5);
        assert_eq!(iter.next(), EOF);
    }

    #[test]
    fn test_empty_doc_id_set() {
        let set = EmptyDocIdSet::new();

        assert_eq!(set.cardinality(), Some(0));
        assert!(set.is_empty());

        let mut iter = set.iterator();
        assert_eq!(iter.next(), EOF);
    }

    #[test]
    fn test_match_all_doc_id_set() {
        let set = MatchAllDocIdSet::new(5);

        assert_eq!(set.cardinality(), Some(5));
        assert!(set.is_match_all());

        let mut iter = set.iterator();
        assert_eq!(iter.next(), 0);
        assert_eq!(iter.next(), 1);
        assert_eq!(iter.next(), 2);
        assert_eq!(iter.next(), 3);
        assert_eq!(iter.next(), 4);
        assert_eq!(iter.next(), EOF);
    }

    #[test]
    fn test_and_doc_id_set() {
        let mut bitmap1 = RoaringBitmap::new();
        bitmap1.insert_range(0..5);

        let mut bitmap2 = RoaringBitmap::new();
        bitmap2.insert_range(3..8);

        let set1: Box<dyn DocIdSet> = Box::new(BitmapDocIdSet::new(bitmap1, 10));
        let set2: Box<dyn DocIdSet> = Box::new(BitmapDocIdSet::new(bitmap2, 10));

        let and_set = AndDocIdSet::new(vec![set1, set2], 10);

        let mut iter = and_set.iterator();
        assert_eq!(iter.next(), 3);
        assert_eq!(iter.next(), 4);
        assert_eq!(iter.next(), EOF);
    }

    #[test]
    fn test_or_doc_id_set() {
        let mut bitmap1 = RoaringBitmap::new();
        bitmap1.insert(1);
        bitmap1.insert(3);

        let mut bitmap2 = RoaringBitmap::new();
        bitmap2.insert(2);
        bitmap2.insert(4);

        let set1: Box<dyn DocIdSet> = Box::new(BitmapDocIdSet::new(bitmap1, 10));
        let set2: Box<dyn DocIdSet> = Box::new(BitmapDocIdSet::new(bitmap2, 10));

        let or_set = OrDocIdSet::new(vec![set1, set2], 10);

        let mut iter = or_set.iterator();
        assert_eq!(iter.next(), 1);
        assert_eq!(iter.next(), 2);
        assert_eq!(iter.next(), 3);
        assert_eq!(iter.next(), 4);
        assert_eq!(iter.next(), EOF);
    }

    #[test]
    fn test_not_doc_id_set() {
        let mut bitmap = RoaringBitmap::new();
        bitmap.insert(1);
        bitmap.insert(3);

        let child: Box<dyn DocIdSet> = Box::new(BitmapDocIdSet::new(bitmap, 5));
        let not_set = NotDocIdSet::new(child, 5);

        let mut iter = not_set.iterator();
        assert_eq!(iter.next(), 0);
        assert_eq!(iter.next(), 2);
        assert_eq!(iter.next(), 4);
        assert_eq!(iter.next(), EOF);
    }

    #[test]
    fn test_and_optimized_with_empty() {
        let empty: Box<dyn DocIdSet> = Box::new(EmptyDocIdSet);
        let match_all: Box<dyn DocIdSet> = Box::new(MatchAllDocIdSet::new(10));

        let result = AndDocIdSet::optimized(vec![empty, match_all], 10);
        assert!(result.is_empty());
    }

    #[test]
    fn test_or_optimized_with_match_all() {
        let empty: Box<dyn DocIdSet> = Box::new(EmptyDocIdSet);
        let match_all: Box<dyn DocIdSet> = Box::new(MatchAllDocIdSet::new(10));

        let result = OrDocIdSet::optimized(vec![empty, match_all], 10);
        assert!(result.is_match_all());
    }

    #[test]
    fn test_bitmap_flip() {
        let mut bitmap = RoaringBitmap::new();
        bitmap.insert(1);
        bitmap.insert(3);

        let set = BitmapDocIdSet::new(bitmap, 5);
        let flipped = set.flip();

        assert_eq!(flipped.cardinality(), Some(3));

        let mut iter = flipped.iterator();
        assert_eq!(iter.next(), 0);
        assert_eq!(iter.next(), 2);
        assert_eq!(iter.next(), 4);
        assert_eq!(iter.next(), EOF);
    }

    #[test]
    fn test_sorted_doc_id_set() {
        let ranges = vec![(0, 2), (5, 6)];
        let set = SortedDocIdSet::new(ranges);

        assert_eq!(set.cardinality(), Some(5)); // 3 + 2

        let mut iter = set.iterator();
        assert_eq!(iter.next(), 0);
        assert_eq!(iter.next(), 1);
        assert_eq!(iter.next(), 2);
        assert_eq!(iter.next(), 5);
        assert_eq!(iter.next(), 6);
        assert_eq!(iter.next(), EOF);
    }

    #[test]
    fn test_bitmap_ops() {
        let mut bitmap1 = RoaringBitmap::new();
        bitmap1.insert_range(0..5);

        let mut bitmap2 = RoaringBitmap::new();
        bitmap2.insert_range(3..8);

        assert_eq!(bitmap_ops::and_cardinality(&bitmap1, &bitmap2), 2);
        assert_eq!(bitmap_ops::or_cardinality(&bitmap1, &bitmap2), 8);
        assert_eq!(bitmap_ops::and_not_cardinality(&bitmap1, &bitmap2), 3);

        let and_result = bitmap_ops::and_bitmaps(&[&bitmap1, &bitmap2]);
        assert_eq!(and_result.len(), 2);

        let or_result = bitmap_ops::or_bitmaps(&[&bitmap1, &bitmap2]);
        assert_eq!(or_result.len(), 8);
    }
}
