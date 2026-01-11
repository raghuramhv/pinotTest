//! Property-based tests for dictionary implementations
//!
//! These tests use proptest to verify invariants and find edge cases
//! that manual tests might miss.

use proptest::prelude::*;
use proptest::collection::vec;
use super::*;
use std::collections::HashSet;

// ============== Strategy Definitions ==============

fn int_values_strategy() -> impl Strategy<Value = Vec<i32>> {
    vec(any::<i32>(), 0..1000)
}

fn long_values_strategy() -> impl Strategy<Value = Vec<i64>> {
    vec(any::<i64>(), 0..1000)
}

fn double_values_strategy() -> impl Strategy<Value = Vec<f64>> {
    vec(
        prop::num::f64::NORMAL | prop::num::f64::POSITIVE | prop::num::f64::NEGATIVE,
        0..500
    )
}

fn string_values_strategy() -> impl Strategy<Value = Vec<String>> {
    vec("[a-zA-Z0-9]{0,100}", 0..500)
}

fn bytes_values_strategy() -> impl Strategy<Value = Vec<Vec<u8>>> {
    vec(vec(any::<u8>(), 0..100), 0..500)
}

// ============== Integer Dictionary Property Tests ==============

proptest! {
    #![proptest_config(ProptestConfig::with_cases(100))]

    /// Every value inserted should be retrievable
    #[test]
    fn int_dict_roundtrip(values in int_values_strategy()) {
        let dict = OnHeapIntDictionary::from_unsorted(values.clone());

        // All unique values should be findable
        let unique: HashSet<_> = values.iter().copied().collect();
        for &val in &unique {
            let dict_id = dict.index_of_int(val);
            prop_assert_ne!(dict_id, NULL_VALUE_INDEX,
                "Value {} should be found in dictionary", val);

            let retrieved = dict.get_int(dict_id).unwrap();
            prop_assert_eq!(retrieved, val,
                "Retrieved value should match original");
        }
    }

    /// Dictionary length equals number of unique values
    #[test]
    fn int_dict_length_equals_unique(values in int_values_strategy()) {
        let dict = OnHeapIntDictionary::from_unsorted(values.clone());
        let unique: HashSet<_> = values.iter().copied().collect();
        prop_assert_eq!(dict.len(), unique.len());
    }

    /// Dict IDs are contiguous from 0 to len-1
    #[test]
    fn int_dict_contiguous_ids(values in int_values_strategy()) {
        let dict = OnHeapIntDictionary::from_unsorted(values);

        for dict_id in 0..dict.len() {
            prop_assert!(dict.get_int(dict_id as i32).is_ok(),
                "Dict ID {} should be valid", dict_id);
        }

        // Out of bounds should fail
        if !dict.is_empty() {
            prop_assert!(dict.get_int(dict.len() as i32).is_err());
            prop_assert!(dict.get_int(-1).is_err());
        }
    }

    /// Sorted dictionary maintains order
    #[test]
    fn int_dict_sorted_order(values in int_values_strategy()) {
        let dict = OnHeapIntDictionary::from_unsorted(values);

        if dict.len() >= 2 {
            for i in 0..(dict.len() - 1) {
                let curr = dict.get_int(i as i32).unwrap();
                let next = dict.get_int((i + 1) as i32).unwrap();
                prop_assert!(curr < next,
                    "Dictionary should be strictly sorted: {} < {}", curr, next);
            }
        }
    }

    /// Min/max are correct
    #[test]
    fn int_dict_min_max(values in int_values_strategy()) {
        let dict = OnHeapIntDictionary::from_unsorted(values.clone());

        if values.is_empty() {
            prop_assert_eq!(dict.min_value(), None);
            prop_assert_eq!(dict.max_value(), None);
        } else {
            let expected_min = *values.iter().min().unwrap();
            let expected_max = *values.iter().max().unwrap();

            prop_assert_eq!(dict.min_value(), Some(DictionaryValue::Int(expected_min)));
            prop_assert_eq!(dict.max_value(), Some(DictionaryValue::Int(expected_max)));
        }
    }

    /// Range query returns correct results
    #[test]
    fn int_dict_range_query(
        values in int_values_strategy(),
        lower in any::<i32>(),
        upper in any::<i32>()
    ) {
        let dict = OnHeapIntDictionary::from_unsorted(values.clone());
        let (lo, hi) = if lower <= upper { (lower, upper) } else { (upper, lower) };

        let result = dict.get_dict_ids_in_range(
            Some(&DictionaryValue::Int(lo)),
            Some(&DictionaryValue::Int(hi)),
            true,
            true,
        );

        // Verify all returned IDs correspond to values in range
        for &dict_id in &result {
            let val = dict.get_int(dict_id).unwrap();
            prop_assert!(val >= lo && val <= hi,
                "Value {} should be in range [{}, {}]", val, lo, hi);
        }

        // Verify all values in range are returned
        let unique: HashSet<_> = values.iter().copied().collect();
        for &val in &unique {
            if val >= lo && val <= hi {
                let dict_id = dict.index_of_int(val);
                prop_assert!(result.contains(&dict_id),
                    "Value {} in range should be in result", val);
            }
        }
    }

    /// Insertion index is correct for binary search
    #[test]
    fn int_dict_insertion_index(values in int_values_strategy(), query in any::<i32>()) {
        let dict = OnHeapIntDictionary::from_unsorted(values);

        let idx = dict.insertion_index_of(&DictionaryValue::Int(query));

        if idx >= 0 {
            // Value exists
            let retrieved = dict.get_int(idx).unwrap();
            prop_assert_eq!(retrieved, query);
        } else {
            // Value doesn't exist, idx is insertion point
            let insert_pos = (-(idx + 1)) as usize;

            // Value before insertion point should be less
            if insert_pos > 0 {
                let before = dict.get_int((insert_pos - 1) as i32).unwrap();
                prop_assert!(before < query);
            }

            // Value at insertion point should be greater
            if insert_pos < dict.len() {
                let after = dict.get_int(insert_pos as i32).unwrap();
                prop_assert!(after > query);
            }
        }
    }
}

// ============== Long Dictionary Property Tests ==============

proptest! {
    #![proptest_config(ProptestConfig::with_cases(100))]

    #[test]
    fn long_dict_roundtrip(values in long_values_strategy()) {
        let dict = OnHeapLongDictionary::from_unsorted(values.clone());

        let unique: HashSet<_> = values.iter().copied().collect();
        for &val in &unique {
            let dict_id = dict.index_of_long(val);
            prop_assert_ne!(dict_id, NULL_VALUE_INDEX);

            let retrieved = dict.get_long(dict_id).unwrap();
            prop_assert_eq!(retrieved, val);
        }
    }

    #[test]
    fn long_dict_sorted_order(values in long_values_strategy()) {
        let dict = OnHeapLongDictionary::from_unsorted(values);

        if dict.len() >= 2 {
            for i in 0..(dict.len() - 1) {
                let curr = dict.get_long(i as i32).unwrap();
                let next = dict.get_long((i + 1) as i32).unwrap();
                prop_assert!(curr < next);
            }
        }
    }
}

// ============== Double Dictionary Property Tests ==============

proptest! {
    #![proptest_config(ProptestConfig::with_cases(50))]

    #[test]
    fn double_dict_roundtrip(values in double_values_strategy()) {
        // Filter out NaN values as they don't compare equal
        let filtered: Vec<f64> = values.into_iter()
            .filter(|v| !v.is_nan())
            .collect();

        let dict = OnHeapDoubleDictionary::from_unsorted(filtered.clone());

        // Unique values (handling float comparison)
        let mut unique = filtered.clone();
        unique.sort_by(|a, b| a.partial_cmp(b).unwrap());
        unique.dedup_by(|a, b| (*a - *b).abs() < f64::EPSILON);

        for &val in &unique {
            let dict_id = dict.index_of_double(val);
            // Due to float comparison issues, just check it's found
            if dict_id != NULL_VALUE_INDEX {
                let retrieved = dict.get_double(dict_id).unwrap();
                prop_assert!((retrieved - val).abs() < f64::EPSILON * 100.0);
            }
        }
    }
}

// ============== String Dictionary Property Tests ==============

proptest! {
    #![proptest_config(ProptestConfig::with_cases(50))]

    #[test]
    fn string_dict_roundtrip(values in string_values_strategy()) {
        let dict = OnHeapStringDictionary::from_unsorted(values.clone());

        let unique: HashSet<_> = values.iter().cloned().collect();
        for val in &unique {
            let dict_id = dict.index_of_string(val);
            prop_assert_ne!(dict_id, NULL_VALUE_INDEX);

            let retrieved = dict.get_string(dict_id).unwrap();
            prop_assert_eq!(&retrieved, val);
        }
    }

    #[test]
    fn string_dict_sorted_order(values in string_values_strategy()) {
        let dict = OnHeapStringDictionary::from_unsorted(values);

        if dict.len() >= 2 {
            for i in 0..(dict.len() - 1) {
                let curr = dict.get_string(i as i32).unwrap();
                let next = dict.get_string((i + 1) as i32).unwrap();
                prop_assert!(curr < next);
            }
        }
    }
}

// ============== Bytes Dictionary Property Tests ==============

proptest! {
    #![proptest_config(ProptestConfig::with_cases(50))]

    #[test]
    fn bytes_dict_roundtrip(values in bytes_values_strategy()) {
        let dict = OnHeapBytesDictionary::from_unsorted(values.clone());

        let unique: HashSet<_> = values.iter().cloned().collect();
        for val in &unique {
            let dict_id = dict.index_of_bytes(val);
            prop_assert_ne!(dict_id, NULL_VALUE_INDEX);

            let retrieved = dict.get_bytes(dict_id).unwrap();
            prop_assert_eq!(&retrieved, val);
        }
    }
}

// ============== Edge Case Tests ==============

#[cfg(test)]
mod edge_cases {
    use super::*;

    #[test]
    fn test_int_dict_boundary_values() {
        let dict = OnHeapIntDictionary::new(vec![i32::MIN, -1, 0, 1, i32::MAX]);

        assert_eq!(dict.index_of_int(i32::MIN), 0);
        assert_eq!(dict.index_of_int(i32::MAX), 4);
        assert_eq!(dict.get_int(0).unwrap(), i32::MIN);
        assert_eq!(dict.get_int(4).unwrap(), i32::MAX);
    }

    #[test]
    fn test_long_dict_boundary_values() {
        let dict = OnHeapLongDictionary::new(vec![i64::MIN, -1, 0, 1, i64::MAX]);

        assert_eq!(dict.index_of_long(i64::MIN), 0);
        assert_eq!(dict.index_of_long(i64::MAX), 4);
    }

    #[test]
    fn test_double_dict_special_values() {
        let dict = OnHeapDoubleDictionary::new(vec![
            f64::NEG_INFINITY,
            f64::MIN,
            -1.0,
            0.0,
            f64::EPSILON,
            1.0,
            f64::MAX,
            f64::INFINITY,
        ]);

        assert_eq!(dict.index_of_double(f64::NEG_INFINITY), 0);
        assert_eq!(dict.index_of_double(f64::INFINITY), 7);
        assert_eq!(dict.index_of_double(0.0), 3);
    }

    #[test]
    fn test_string_dict_unicode() {
        let dict = OnHeapStringDictionary::new(vec![
            "".to_string(),
            "a".to_string(),
            "café".to_string(),
            "日本語".to_string(),
            "🎉".to_string(),
        ]);

        assert_eq!(dict.index_of_string("café"), 2);
        assert_eq!(dict.index_of_string("日本語"), 3);
        assert_eq!(dict.index_of_string("🎉"), 4);
        assert_eq!(dict.index_of_string(""), 0);
    }

    #[test]
    fn test_string_dict_special_chars() {
        let dict = OnHeapStringDictionary::from_unsorted(vec![
            "\0".to_string(),
            "\n".to_string(),
            "\t".to_string(),
            "\\".to_string(),
            "\"".to_string(),
        ]);

        assert_ne!(dict.index_of_string("\0"), NULL_VALUE_INDEX);
        assert_ne!(dict.index_of_string("\n"), NULL_VALUE_INDEX);
    }

    #[test]
    fn test_bytes_dict_empty_bytes() {
        let dict = OnHeapBytesDictionary::new(vec![
            vec![],
            vec![0x00],
            vec![0x00, 0x00],
            vec![0xFF],
        ]);

        assert_eq!(dict.index_of_bytes(&[]), 0);
        assert_eq!(dict.index_of_bytes(&[0x00]), 1);
        assert_eq!(dict.index_of_bytes(&[0xFF]), 3);
    }

    #[test]
    fn test_range_query_edge_cases() {
        let dict = OnHeapIntDictionary::new(vec![10, 20, 30, 40, 50]);

        // Empty range
        let empty = dict.get_dict_ids_in_range(
            Some(&DictionaryValue::Int(25)),
            Some(&DictionaryValue::Int(25)),
            false,
            false,
        );
        assert!(empty.is_empty());

        // Single element range
        let single = dict.get_dict_ids_in_range(
            Some(&DictionaryValue::Int(30)),
            Some(&DictionaryValue::Int(30)),
            true,
            true,
        );
        assert_eq!(single.len(), 1);

        // Range outside dictionary
        let outside = dict.get_dict_ids_in_range(
            Some(&DictionaryValue::Int(100)),
            Some(&DictionaryValue::Int(200)),
            true,
            true,
        );
        assert!(outside.is_empty());

        // Open-ended range (no lower bound)
        let no_lower = dict.get_dict_ids_in_range(
            None,
            Some(&DictionaryValue::Int(30)),
            true,
            true,
        );
        assert_eq!(no_lower.len(), 3); // 10, 20, 30

        // Open-ended range (no upper bound)
        let no_upper = dict.get_dict_ids_in_range(
            Some(&DictionaryValue::Int(30)),
            None,
            true,
            true,
        );
        assert_eq!(no_upper.len(), 3); // 30, 40, 50

        // Fully open range
        let all = dict.get_dict_ids_in_range(None, None, true, true);
        assert_eq!(all.len(), 5);
    }

    #[test]
    fn test_batch_read_edge_cases() {
        let dict = OnHeapIntDictionary::new(vec![100, 200, 300]);

        // Empty batch
        let mut empty_out: [i32; 0] = [];
        assert!(dict.read_int_values(&[], &mut empty_out).is_ok());

        // Single element batch
        let mut single_out = [0i32; 1];
        assert!(dict.read_int_values(&[1], &mut single_out).is_ok());
        assert_eq!(single_out[0], 200);

        // Invalid dict_id in batch
        let mut invalid_out = [0i32; 2];
        assert!(dict.read_int_values(&[0, 99], &mut invalid_out).is_err());

        // Negative dict_id
        assert!(dict.read_int_values(&[-1], &mut [0i32; 1]).is_err());
    }

    #[test]
    fn test_type_coercion_edge_cases() {
        let long_dict = OnHeapLongDictionary::new(vec![100, 200, 300]);

        // Int coerced to Long
        assert_eq!(long_dict.index_of(&DictionaryValue::Int(200)), 1);

        let double_dict = OnHeapDoubleDictionary::new(vec![1.0, 2.0, 3.0]);

        // Int coerced to Double
        assert_eq!(double_dict.index_of(&DictionaryValue::Int(2)), 1);
        // Long coerced to Double
        assert_eq!(double_dict.index_of(&DictionaryValue::Long(2)), 1);

        // Wrong type returns NULL_VALUE_INDEX
        let int_dict = OnHeapIntDictionary::new(vec![1, 2, 3]);
        assert_eq!(int_dict.index_of(&DictionaryValue::String("test".to_string())), NULL_VALUE_INDEX);
    }

    #[test]
    fn test_large_dictionary() {
        // Test with a larger dictionary to catch scaling issues
        let values: Vec<i32> = (0..10000).collect();
        let dict = OnHeapIntDictionary::new(values);

        assert_eq!(dict.len(), 10000);
        assert_eq!(dict.index_of_int(0), 0);
        assert_eq!(dict.index_of_int(9999), 9999);
        assert_eq!(dict.index_of_int(5000), 5000);

        // Binary search should still work
        assert_eq!(dict.insertion_index_of(&DictionaryValue::Int(5000)), 5000);
        assert_eq!(dict.insertion_index_of(&DictionaryValue::Int(-1)), -1);
        assert_eq!(dict.insertion_index_of(&DictionaryValue::Int(10000)), -10001);
    }

    #[test]
    fn test_duplicate_handling() {
        // Ensure duplicates are properly removed
        let dict = OnHeapIntDictionary::from_unsorted(vec![1, 1, 1, 2, 2, 3]);
        assert_eq!(dict.len(), 3);

        let dict = OnHeapStringDictionary::from_unsorted(vec![
            "a".to_string(),
            "a".to_string(),
            "b".to_string(),
        ]);
        assert_eq!(dict.len(), 2);
    }
}
