//! Document ID iterators
//!
//! Provides iterator implementations for traversing document IDs.

use super::EOF;
use roaring::RoaringBitmap;

/// Trait for document ID iterators
pub trait DocIdIterator: Send {
    /// Returns the next document ID, or EOF if exhausted
    fn next(&mut self) -> i32;

    /// Advances to the target document ID and returns it, or the next ID >= target
    fn advance(&mut self, target: i32) -> i32;

    /// Returns the number of entries scanned
    fn num_entries_scanned(&self) -> u64 {
        0
    }
}

/// Iterator over a RoaringBitmap
pub struct BitmapDocIdIterator {
    bitmap: RoaringBitmap,
    iter: roaring::bitmap::IntoIter,
    num_docs: i32,
    current: i32,
}

impl BitmapDocIdIterator {
    pub fn new(bitmap: RoaringBitmap, num_docs: i32) -> Self {
        let iter = bitmap.clone().into_iter();
        Self {
            bitmap,
            iter,
            num_docs,
            current: -1,
        }
    }

    /// Returns the underlying bitmap
    pub fn bitmap(&self) -> &RoaringBitmap {
        &self.bitmap
    }
}

impl DocIdIterator for BitmapDocIdIterator {
    fn next(&mut self) -> i32 {
        match self.iter.next() {
            Some(doc_id) if (doc_id as i32) < self.num_docs => {
                self.current = doc_id as i32;
                self.current
            }
            _ => EOF,
        }
    }

    fn advance(&mut self, target: i32) -> i32 {
        if target <= self.current {
            return self.current;
        }

        // Skip to target
        while let Some(doc_id) = self.iter.next() {
            let doc_id = doc_id as i32;
            if doc_id >= target && doc_id < self.num_docs {
                self.current = doc_id;
                return doc_id;
            }
            if doc_id >= self.num_docs {
                break;
            }
        }
        EOF
    }
}

/// Iterator that matches all documents in a range [0, num_docs)
pub struct MatchAllDocIdIterator {
    num_docs: i32,
    current: i32,
}

impl MatchAllDocIdIterator {
    pub fn new(num_docs: i32) -> Self {
        Self {
            num_docs,
            current: -1,
        }
    }
}

impl DocIdIterator for MatchAllDocIdIterator {
    fn next(&mut self) -> i32 {
        self.current += 1;
        if self.current < self.num_docs {
            self.current
        } else {
            EOF
        }
    }

    fn advance(&mut self, target: i32) -> i32 {
        if target < self.num_docs {
            self.current = target;
            self.current
        } else {
            EOF
        }
    }
}

/// Iterator that matches no documents
pub struct EmptyDocIdIterator;

impl EmptyDocIdIterator {
    pub fn new() -> Self {
        Self
    }
}

impl Default for EmptyDocIdIterator {
    fn default() -> Self {
        Self::new()
    }
}

impl DocIdIterator for EmptyDocIdIterator {
    fn next(&mut self) -> i32 {
        EOF
    }

    fn advance(&mut self, _target: i32) -> i32 {
        EOF
    }
}

/// AND iterator - returns documents matching all child iterators
pub struct AndDocIdIterator {
    iterators: Vec<Box<dyn DocIdIterator>>,
    current_doc_ids: Vec<i32>,
    exhausted: bool,
}

impl AndDocIdIterator {
    pub fn new(mut iterators: Vec<Box<dyn DocIdIterator>>) -> Self {
        let mut current_doc_ids = Vec::with_capacity(iterators.len());

        // Initialize all iterators
        for iter in iterators.iter_mut() {
            current_doc_ids.push(iter.next());
        }

        // Check if any is already exhausted
        let exhausted = current_doc_ids.iter().any(|&id| id == EOF);

        Self {
            iterators,
            current_doc_ids,
            exhausted,
        }
    }
}

impl DocIdIterator for AndDocIdIterator {
    fn next(&mut self) -> i32 {
        if self.exhausted || self.iterators.is_empty() {
            return EOF;
        }

        loop {
            // Find the maximum current doc ID
            let max_doc_id = *self.current_doc_ids.iter().max().unwrap_or(&EOF);

            if max_doc_id == EOF {
                self.exhausted = true;
                return EOF;
            }

            // Try to advance all iterators to the max
            let mut all_equal = true;
            for (i, iter) in self.iterators.iter_mut().enumerate() {
                if self.current_doc_ids[i] < max_doc_id {
                    self.current_doc_ids[i] = iter.advance(max_doc_id);
                    if self.current_doc_ids[i] == EOF {
                        self.exhausted = true;
                        return EOF;
                    }
                    if self.current_doc_ids[i] != max_doc_id {
                        all_equal = false;
                    }
                }
            }

            if all_equal {
                // All iterators are at the same doc ID
                let result = max_doc_id;

                // Advance all for next call
                for (i, iter) in self.iterators.iter_mut().enumerate() {
                    self.current_doc_ids[i] = iter.next();
                }

                return result;
            }
        }
    }

    fn advance(&mut self, target: i32) -> i32 {
        if self.exhausted {
            return EOF;
        }

        // Advance all iterators to at least target
        for (i, iter) in self.iterators.iter_mut().enumerate() {
            if self.current_doc_ids[i] < target {
                self.current_doc_ids[i] = iter.advance(target);
                if self.current_doc_ids[i] == EOF {
                    self.exhausted = true;
                    return EOF;
                }
            }
        }

        // Now find the intersection
        self.next()
    }
}

/// OR iterator - returns documents matching any child iterator
pub struct OrDocIdIterator {
    iterators: Vec<Box<dyn DocIdIterator>>,
    current_doc_ids: Vec<i32>,
    previous_doc_id: i32,
    num_active: usize,
}

impl OrDocIdIterator {
    pub fn new(mut iterators: Vec<Box<dyn DocIdIterator>>) -> Self {
        let mut current_doc_ids = Vec::with_capacity(iterators.len());

        for iter in iterators.iter_mut() {
            current_doc_ids.push(iter.next());
        }

        let num_active = current_doc_ids.iter().filter(|&&id| id != EOF).count();

        Self {
            iterators,
            current_doc_ids,
            previous_doc_id: -1,
            num_active,
        }
    }
}

impl DocIdIterator for OrDocIdIterator {
    fn next(&mut self) -> i32 {
        if self.num_active == 0 {
            return EOF;
        }

        // First pass: advance iterators that were at previous doc ID
        for i in 0..self.current_doc_ids.len() {
            let doc_id = self.current_doc_ids[i];
            if doc_id == self.previous_doc_id && doc_id != EOF {
                // This iterator was at previous, advance it
                self.current_doc_ids[i] = self.iterators[i].next();
                if self.current_doc_ids[i] == EOF {
                    self.num_active -= 1;
                }
            }
        }

        // Second pass: find minimum doc ID
        let mut next_doc_id = i32::MAX;
        for &doc_id in self.current_doc_ids.iter() {
            if doc_id != EOF && doc_id < next_doc_id {
                next_doc_id = doc_id;
            }
        }

        if next_doc_id == i32::MAX {
            EOF
        } else {
            self.previous_doc_id = next_doc_id;
            next_doc_id
        }
    }

    fn advance(&mut self, target: i32) -> i32 {
        if self.num_active == 0 {
            return EOF;
        }

        let mut next_doc_id = i32::MAX;

        // Advance all iterators to at least target
        for (i, iter) in self.iterators.iter_mut().enumerate() {
            if self.current_doc_ids[i] != EOF && self.current_doc_ids[i] < target {
                self.current_doc_ids[i] = iter.advance(target);
                if self.current_doc_ids[i] == EOF {
                    self.num_active -= 1;
                }
            }

            if self.current_doc_ids[i] != EOF && self.current_doc_ids[i] < next_doc_id {
                next_doc_id = self.current_doc_ids[i];
            }
        }

        if next_doc_id == i32::MAX {
            EOF
        } else {
            self.previous_doc_id = next_doc_id;
            next_doc_id
        }
    }
}

/// NOT iterator - returns documents NOT in the child iterator
pub struct NotDocIdIterator {
    child: Box<dyn DocIdIterator>,
    num_docs: i32,
    current: i32,
    next_excluded: i32,
}

impl NotDocIdIterator {
    pub fn new(mut child: Box<dyn DocIdIterator>, num_docs: i32) -> Self {
        let next_excluded = child.next();
        Self {
            child,
            num_docs,
            current: -1,
            next_excluded,
        }
    }
}

impl DocIdIterator for NotDocIdIterator {
    fn next(&mut self) -> i32 {
        loop {
            self.current += 1;

            if self.current >= self.num_docs {
                return EOF;
            }

            // Skip excluded doc IDs
            while self.current == self.next_excluded {
                self.current += 1;
                self.next_excluded = self.child.next();

                if self.current >= self.num_docs {
                    return EOF;
                }
            }

            return self.current;
        }
    }

    fn advance(&mut self, target: i32) -> i32 {
        if target >= self.num_docs {
            return EOF;
        }

        self.current = target - 1;

        // Advance child iterator if needed
        if target > self.next_excluded {
            self.next_excluded = self.child.advance(target);
        }

        self.next()
    }
}

/// Iterator over sorted document ID ranges
pub struct SortedDocIdIterator {
    ranges: Vec<(i32, i32)>, // (start, end) inclusive
    current_range_idx: usize,
    current: i32,
}

impl SortedDocIdIterator {
    pub fn new(ranges: Vec<(i32, i32)>) -> Self {
        let current = if ranges.is_empty() {
            -1
        } else {
            ranges[0].0 - 1
        };

        Self {
            ranges,
            current_range_idx: 0,
            current,
        }
    }

    /// Returns the ranges
    pub fn ranges(&self) -> &[(i32, i32)] {
        &self.ranges
    }
}

impl DocIdIterator for SortedDocIdIterator {
    fn next(&mut self) -> i32 {
        if self.current_range_idx >= self.ranges.len() {
            return EOF;
        }

        let (_, end) = self.ranges[self.current_range_idx];

        self.current += 1;

        if self.current <= end {
            return self.current;
        }

        // Move to next range
        self.current_range_idx += 1;
        if self.current_range_idx >= self.ranges.len() {
            return EOF;
        }

        let (start, _) = self.ranges[self.current_range_idx];
        self.current = start;
        self.current
    }

    fn advance(&mut self, target: i32) -> i32 {
        // Find the range containing target
        while self.current_range_idx < self.ranges.len() {
            let (start, end) = self.ranges[self.current_range_idx];

            if target <= end {
                self.current = target.max(start);
                return self.current;
            }

            self.current_range_idx += 1;
        }

        EOF
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_bitmap_iterator() {
        let mut bitmap = RoaringBitmap::new();
        bitmap.insert(1);
        bitmap.insert(3);
        bitmap.insert(5);
        bitmap.insert(7);

        let mut iter = BitmapDocIdIterator::new(bitmap, 10);

        assert_eq!(iter.next(), 1);
        assert_eq!(iter.next(), 3);
        assert_eq!(iter.next(), 5);
        assert_eq!(iter.next(), 7);
        assert_eq!(iter.next(), EOF);
    }

    #[test]
    fn test_bitmap_iterator_advance() {
        let mut bitmap = RoaringBitmap::new();
        for i in 0..100 {
            if i % 10 == 0 {
                bitmap.insert(i);
            }
        }

        let mut iter = BitmapDocIdIterator::new(bitmap, 100);

        assert_eq!(iter.advance(25), 30);
        assert_eq!(iter.next(), 40);
    }

    #[test]
    fn test_match_all_iterator() {
        let mut iter = MatchAllDocIdIterator::new(5);

        assert_eq!(iter.next(), 0);
        assert_eq!(iter.next(), 1);
        assert_eq!(iter.next(), 2);
        assert_eq!(iter.next(), 3);
        assert_eq!(iter.next(), 4);
        assert_eq!(iter.next(), EOF);
    }

    #[test]
    fn test_match_all_advance() {
        let mut iter = MatchAllDocIdIterator::new(10);

        assert_eq!(iter.advance(5), 5);
        assert_eq!(iter.next(), 6);
    }

    #[test]
    fn test_empty_iterator() {
        let mut iter = EmptyDocIdIterator::new();

        assert_eq!(iter.next(), EOF);
        assert_eq!(iter.advance(10), EOF);
    }

    #[test]
    fn test_and_iterator() {
        let mut bitmap1 = RoaringBitmap::new();
        bitmap1.insert(1);
        bitmap1.insert(3);
        bitmap1.insert(5);
        bitmap1.insert(7);

        let mut bitmap2 = RoaringBitmap::new();
        bitmap2.insert(2);
        bitmap2.insert(3);
        bitmap2.insert(5);
        bitmap2.insert(8);

        let iter1: Box<dyn DocIdIterator> = Box::new(BitmapDocIdIterator::new(bitmap1, 10));
        let iter2: Box<dyn DocIdIterator> = Box::new(BitmapDocIdIterator::new(bitmap2, 10));

        let mut and_iter = AndDocIdIterator::new(vec![iter1, iter2]);

        assert_eq!(and_iter.next(), 3);
        assert_eq!(and_iter.next(), 5);
        assert_eq!(and_iter.next(), EOF);
    }

    #[test]
    fn test_or_iterator() {
        let mut bitmap1 = RoaringBitmap::new();
        bitmap1.insert(1);
        bitmap1.insert(3);

        let mut bitmap2 = RoaringBitmap::new();
        bitmap2.insert(2);
        bitmap2.insert(3);

        let iter1: Box<dyn DocIdIterator> = Box::new(BitmapDocIdIterator::new(bitmap1, 10));
        let iter2: Box<dyn DocIdIterator> = Box::new(BitmapDocIdIterator::new(bitmap2, 10));

        let mut or_iter = OrDocIdIterator::new(vec![iter1, iter2]);

        assert_eq!(or_iter.next(), 1);
        assert_eq!(or_iter.next(), 2);
        assert_eq!(or_iter.next(), 3);
        assert_eq!(or_iter.next(), EOF);
    }

    #[test]
    fn test_not_iterator() {
        let mut bitmap = RoaringBitmap::new();
        bitmap.insert(1);
        bitmap.insert(3);

        let child: Box<dyn DocIdIterator> = Box::new(BitmapDocIdIterator::new(bitmap, 5));
        let mut not_iter = NotDocIdIterator::new(child, 5);

        assert_eq!(not_iter.next(), 0);
        assert_eq!(not_iter.next(), 2);
        assert_eq!(not_iter.next(), 4);
        assert_eq!(not_iter.next(), EOF);
    }

    #[test]
    fn test_sorted_iterator() {
        let ranges = vec![(0, 2), (5, 7), (10, 10)];
        let mut iter = SortedDocIdIterator::new(ranges);

        assert_eq!(iter.next(), 0);
        assert_eq!(iter.next(), 1);
        assert_eq!(iter.next(), 2);
        assert_eq!(iter.next(), 5);
        assert_eq!(iter.next(), 6);
        assert_eq!(iter.next(), 7);
        assert_eq!(iter.next(), 10);
        assert_eq!(iter.next(), EOF);
    }

    #[test]
    fn test_sorted_iterator_advance() {
        let ranges = vec![(0, 5), (10, 15), (20, 25)];
        let mut iter = SortedDocIdIterator::new(ranges);

        assert_eq!(iter.advance(12), 12);
        assert_eq!(iter.next(), 13);
        assert_eq!(iter.advance(22), 22);
    }

    #[test]
    fn test_complex_and_or() {
        // (A AND B) where A = [1,2,3,4,5] and B = [3,4,5,6,7]
        let mut bitmap_a = RoaringBitmap::new();
        for i in 1..=5 {
            bitmap_a.insert(i);
        }

        let mut bitmap_b = RoaringBitmap::new();
        for i in 3..=7 {
            bitmap_b.insert(i);
        }

        let iter_a: Box<dyn DocIdIterator> = Box::new(BitmapDocIdIterator::new(bitmap_a, 10));
        let iter_b: Box<dyn DocIdIterator> = Box::new(BitmapDocIdIterator::new(bitmap_b, 10));

        let mut and_iter = AndDocIdIterator::new(vec![iter_a, iter_b]);

        let mut results = Vec::new();
        loop {
            let doc_id = and_iter.next();
            if doc_id == EOF {
                break;
            }
            results.push(doc_id);
        }

        assert_eq!(results, vec![3, 4, 5]);
    }
}
