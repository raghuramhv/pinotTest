//! JNI bindings for calling Rust from Java
//!
//! This module provides JNI-compatible functions that can be called from Java.
//!
//! # Usage
//!
//! To use these functions from Java:
//!
//! 1. Compile this crate as a cdylib:
//!    ```bash
//!    cargo build --release
//!    ```
//!
//! 2. Load the library in Java:
//!    ```java
//!    System.loadLibrary("pinot_core");
//!    ```
//!
//! 3. Declare native methods:
//!    ```java
//!    public class PinotRustBridge {
//!        static { System.loadLibrary("pinot_core"); }
//!
//!        public static native double sumDoubleArray(double[] values);
//!        public static native long countNonNull(double[] values, long[] nullIndices);
//!        // ...
//!    }
//!    ```

use jni::JNIEnv;
use jni::objects::{JClass, JDoubleArray, JIntArray, JLongArray};
use jni::sys::{jdouble, jint, jlong};

use crate::aggregation::*;
use crate::dictionary::*;
use crate::bitmap::*;

// ============================================================================
// Aggregation Functions
// ============================================================================

/// Computes the sum of a double array
#[no_mangle]
pub extern "system" fn Java_org_apache_pinot_core_rust_PinotRustBridge_sumDoubleArray<'local>(
    env: JNIEnv<'local>,
    _class: JClass<'local>,
    array: JDoubleArray<'local>,
) -> jdouble {
    let len = match env.get_array_length(&array) {
        Ok(l) => l as usize,
        Err(_) => return 0.0,
    };

    let mut values = vec![0.0f64; len];
    if env.get_double_array_region(&array, 0, &mut values).is_err() {
        return 0.0;
    }

    let func = SumAggregationFunction::new();
    let mut holder = func.create_result_holder();
    func.aggregate_double(&values, holder.as_mut(), None);
    holder.get_double()
}

/// Computes the count of elements
#[no_mangle]
pub extern "system" fn Java_org_apache_pinot_core_rust_PinotRustBridge_countDoubleArray<'local>(
    env: JNIEnv<'local>,
    _class: JClass<'local>,
    array: JDoubleArray<'local>,
) -> jlong {
    let len = match env.get_array_length(&array) {
        Ok(l) => l as usize,
        Err(_) => return 0,
    };

    len as jlong
}

/// Computes the minimum value
#[no_mangle]
pub extern "system" fn Java_org_apache_pinot_core_rust_PinotRustBridge_minDoubleArray<'local>(
    env: JNIEnv<'local>,
    _class: JClass<'local>,
    array: JDoubleArray<'local>,
) -> jdouble {
    let len = match env.get_array_length(&array) {
        Ok(l) => l as usize,
        Err(_) => return f64::INFINITY,
    };

    let mut values = vec![0.0f64; len];
    if env.get_double_array_region(&array, 0, &mut values).is_err() {
        return f64::INFINITY;
    }

    let func = MinAggregationFunction::new();
    let mut holder = func.create_result_holder();
    func.aggregate_double(&values, holder.as_mut(), None);
    holder.get_double()
}

/// Computes the maximum value
#[no_mangle]
pub extern "system" fn Java_org_apache_pinot_core_rust_PinotRustBridge_maxDoubleArray<'local>(
    env: JNIEnv<'local>,
    _class: JClass<'local>,
    array: JDoubleArray<'local>,
) -> jdouble {
    let len = match env.get_array_length(&array) {
        Ok(l) => l as usize,
        Err(_) => return f64::NEG_INFINITY,
    };

    let mut values = vec![0.0f64; len];
    if env.get_double_array_region(&array, 0, &mut values).is_err() {
        return f64::NEG_INFINITY;
    }

    let func = MaxAggregationFunction::new();
    let mut holder = func.create_result_holder();
    func.aggregate_double(&values, holder.as_mut(), None);
    holder.get_double()
}

/// Computes the average value
#[no_mangle]
pub extern "system" fn Java_org_apache_pinot_core_rust_PinotRustBridge_avgDoubleArray<'local>(
    env: JNIEnv<'local>,
    _class: JClass<'local>,
    array: JDoubleArray<'local>,
) -> jdouble {
    let len = match env.get_array_length(&array) {
        Ok(l) => l as usize,
        Err(_) => return 0.0,
    };

    let mut values = vec![0.0f64; len];
    if env.get_double_array_region(&array, 0, &mut values).is_err() {
        return 0.0;
    }

    let func = AvgAggregationFunction::new();
    let mut holder = func.create_result_holder();
    func.aggregate_double(&values, holder.as_mut(), None);
    holder.get_double()
}

// ============================================================================
// Dictionary Functions
// ============================================================================

/// Creates a dictionary from int values and returns a handle
/// The handle can be used in subsequent calls
#[no_mangle]
pub extern "system" fn Java_org_apache_pinot_core_rust_PinotRustBridge_createIntDictionary<'local>(
    env: JNIEnv<'local>,
    _class: JClass<'local>,
    values: JIntArray<'local>,
) -> jlong {
    let len = match env.get_array_length(&values) {
        Ok(l) => l as usize,
        Err(_) => return 0,
    };

    let mut rust_values = vec![0i32; len];
    if env.get_int_array_region(&values, 0, &mut rust_values).is_err() {
        return 0;
    }

    let dict = Box::new(OnHeapIntDictionary::from_unsorted(rust_values));
    Box::into_raw(dict) as jlong
}

/// Looks up a value in an int dictionary
#[no_mangle]
pub extern "system" fn Java_org_apache_pinot_core_rust_PinotRustBridge_intDictionaryIndexOf(
    _env: JNIEnv,
    _class: JClass,
    handle: jlong,
    value: jint,
) -> jint {
    if handle == 0 {
        return -1;
    }

    let dict = unsafe { &*(handle as *const OnHeapIntDictionary) };
    dict.index_of_int(value)
}

/// Gets a value from an int dictionary by ID
#[no_mangle]
pub extern "system" fn Java_org_apache_pinot_core_rust_PinotRustBridge_intDictionaryGet(
    _env: JNIEnv,
    _class: JClass,
    handle: jlong,
    dict_id: jint,
) -> jint {
    if handle == 0 {
        return 0;
    }

    let dict = unsafe { &*(handle as *const OnHeapIntDictionary) };
    dict.get_int(dict_id).unwrap_or(0)
}

/// Frees an int dictionary
#[no_mangle]
pub extern "system" fn Java_org_apache_pinot_core_rust_PinotRustBridge_freeIntDictionary(
    _env: JNIEnv,
    _class: JClass,
    handle: jlong,
) {
    if handle != 0 {
        unsafe {
            drop(Box::from_raw(handle as *mut OnHeapIntDictionary));
        }
    }
}

// ============================================================================
// Bitmap Functions
// ============================================================================

/// Computes AND of two bitmaps and returns cardinality
#[no_mangle]
pub extern "system" fn Java_org_apache_pinot_core_rust_PinotRustBridge_bitmapAndCardinality<'local>(
    env: JNIEnv<'local>,
    _class: JClass<'local>,
    bitmap1: JLongArray<'local>,
    bitmap2: JLongArray<'local>,
) -> jlong {
    // This is a simplified version - in practice you'd serialize RoaringBitmaps
    let len1 = env.get_array_length(&bitmap1).unwrap_or(0) as usize;
    let len2 = env.get_array_length(&bitmap2).unwrap_or(0) as usize;

    // For demonstration, treat longs as doc IDs
    let mut ids1 = vec![0i64; len1];
    let mut ids2 = vec![0i64; len2];

    let _ = env.get_long_array_region(&bitmap1, 0, &mut ids1);
    let _ = env.get_long_array_region(&bitmap2, 0, &mut ids2);

    let mut bm1 = roaring::RoaringBitmap::new();
    let mut bm2 = roaring::RoaringBitmap::new();

    for id in ids1 {
        if id >= 0 {
            bm1.insert(id as u32);
        }
    }
    for id in ids2 {
        if id >= 0 {
            bm2.insert(id as u32);
        }
    }

    bitmap_ops::and_cardinality(&bm1, &bm2) as jlong
}

/// Computes OR of two bitmaps and returns cardinality
#[no_mangle]
pub extern "system" fn Java_org_apache_pinot_core_rust_PinotRustBridge_bitmapOrCardinality<'local>(
    env: JNIEnv<'local>,
    _class: JClass<'local>,
    bitmap1: JLongArray<'local>,
    bitmap2: JLongArray<'local>,
) -> jlong {
    let len1 = env.get_array_length(&bitmap1).unwrap_or(0) as usize;
    let len2 = env.get_array_length(&bitmap2).unwrap_or(0) as usize;

    let mut ids1 = vec![0i64; len1];
    let mut ids2 = vec![0i64; len2];

    let _ = env.get_long_array_region(&bitmap1, 0, &mut ids1);
    let _ = env.get_long_array_region(&bitmap2, 0, &mut ids2);

    let mut bm1 = roaring::RoaringBitmap::new();
    let mut bm2 = roaring::RoaringBitmap::new();

    for id in ids1 {
        if id >= 0 {
            bm1.insert(id as u32);
        }
    }
    for id in ids2 {
        if id >= 0 {
            bm2.insert(id as u32);
        }
    }

    bitmap_ops::or_cardinality(&bm1, &bm2) as jlong
}

#[cfg(test)]
mod tests {
    // JNI tests require a JVM, so we just verify compilation
    #[test]
    fn test_jni_compilation() {
        // This test just ensures the JNI module compiles correctly
        assert!(true);
    }
}
