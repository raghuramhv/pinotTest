//! JNI bindings for calling Rust from Java
//!
//! This module provides JNI-compatible functions that can be called from Java code.
//! Function names follow the JNI naming convention: Java_package_class_method

#![allow(unused_mut)]

use jni::objects::{JByteArray, JClass, JDoubleArray, JIntArray, JLongArray, JObject, JObjectArray, JString};
use jni::sys::{jdouble, jint, jlong};
use jni::JNIEnv;
use std::collections::HashMap;
use std::sync::Mutex as StdMutex;

use crate::dictionary::{OnHeapIntDictionary, OnHeapLongDictionary, OnHeapStringDictionary, Dictionary};

// Global storage for dictionary handles
lazy_static::lazy_static! {
    static ref INT_DICTIONARIES: StdMutex<HashMap<jlong, OnHeapIntDictionary>> = StdMutex::new(HashMap::new());
    static ref LONG_DICTIONARIES: StdMutex<HashMap<jlong, OnHeapLongDictionary>> = StdMutex::new(HashMap::new());
    static ref STRING_DICTIONARIES: StdMutex<HashMap<jlong, OnHeapStringDictionary>> = StdMutex::new(HashMap::new());
    static ref NEXT_HANDLE: StdMutex<jlong> = StdMutex::new(1);
}

fn get_next_handle() -> jlong {
    let mut handle = NEXT_HANDLE.lock().unwrap();
    let current = *handle;
    *handle += 1;
    current
}

// ==================== Aggregation Functions ====================

/// Sum of double array
#[no_mangle]
pub extern "system" fn Java_org_apache_pinot_core_native_NativeAggregation_nativeSumDoubleArray<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
    values: JDoubleArray<'local>,
) -> jdouble {
    let len = env.get_array_length(&values).unwrap_or(0) as usize;
    if len == 0 {
        return 0.0;
    }

    let mut buffer = vec![0.0f64; len];
    if env.get_double_array_region(&values, 0, &mut buffer).is_err() {
        return 0.0;
    }

    buffer.iter().sum()
}

/// Sum of double array with null handling
#[no_mangle]
pub extern "system" fn Java_org_apache_pinot_core_native_NativeAggregation_nativeSumDoubleArrayWithNulls<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
    values: JDoubleArray<'local>,
    null_bitmap: JLongArray<'local>,
) -> JObject<'local> {
    let len = env.get_array_length(&values).unwrap_or(0) as usize;
    if len == 0 {
        return JObject::null();
    }

    let mut buffer = vec![0.0f64; len];
    if env.get_double_array_region(&values, 0, &mut buffer).is_err() {
        return JObject::null();
    }

    // Get null bitmap if provided
    let null_indices: Vec<i64> = if !null_bitmap.is_null() {
        let null_len = env.get_array_length(&null_bitmap).unwrap_or(0) as usize;
        if null_len > 0 {
            let mut null_buffer = vec![0i64; null_len];
            if env.get_long_array_region(&null_bitmap, 0, &mut null_buffer).is_ok() {
                null_buffer
            } else {
                vec![]
            }
        } else {
            vec![]
        }
    } else {
        vec![]
    };

    let null_set: std::collections::HashSet<usize> = null_indices.iter().map(|&x| x as usize).collect();

    let mut sum = 0.0;
    let mut has_value = false;
    for (i, &v) in buffer.iter().enumerate() {
        if !null_set.contains(&i) {
            sum += v;
            has_value = true;
        }
    }

    if !has_value {
        return JObject::null();
    }

    // Return boxed Double
    let double_class = env.find_class("java/lang/Double").unwrap();
    env.new_object(double_class, "(D)V", &[jni::objects::JValue::Double(sum)])
        .unwrap_or(JObject::null())
}

/// Sum of int array
#[no_mangle]
pub extern "system" fn Java_org_apache_pinot_core_native_NativeAggregation_nativeSumIntArray<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
    values: JIntArray<'local>,
) -> jlong {
    let len = env.get_array_length(&values).unwrap_or(0) as usize;
    if len == 0 {
        return 0;
    }

    let mut buffer = vec![0i32; len];
    if env.get_int_array_region(&values, 0, &mut buffer).is_err() {
        return 0;
    }

    buffer.iter().map(|&x| x as i64).sum()
}

/// Sum of long array
#[no_mangle]
pub extern "system" fn Java_org_apache_pinot_core_native_NativeAggregation_nativeSumLongArray<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
    values: JLongArray<'local>,
) -> jlong {
    let len = env.get_array_length(&values).unwrap_or(0) as usize;
    if len == 0 {
        return 0;
    }

    let mut buffer = vec![0i64; len];
    if env.get_long_array_region(&values, 0, &mut buffer).is_err() {
        return 0;
    }

    buffer.iter().sum()
}

/// Min of double array
#[no_mangle]
pub extern "system" fn Java_org_apache_pinot_core_native_NativeAggregation_nativeMinDoubleArray<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
    values: JDoubleArray<'local>,
) -> jdouble {
    let len = env.get_array_length(&values).unwrap_or(0) as usize;
    if len == 0 {
        return f64::MAX;
    }

    let mut buffer = vec![0.0f64; len];
    if env.get_double_array_region(&values, 0, &mut buffer).is_err() {
        return f64::MAX;
    }

    buffer.iter().cloned().fold(f64::MAX, f64::min)
}

/// Min of double array with nulls
#[no_mangle]
pub extern "system" fn Java_org_apache_pinot_core_native_NativeAggregation_nativeMinDoubleArrayWithNulls<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
    values: JDoubleArray<'local>,
    null_bitmap: JLongArray<'local>,
) -> JObject<'local> {
    let len = env.get_array_length(&values).unwrap_or(0) as usize;
    if len == 0 {
        return JObject::null();
    }

    let mut buffer = vec![0.0f64; len];
    if env.get_double_array_region(&values, 0, &mut buffer).is_err() {
        return JObject::null();
    }

    let null_indices: Vec<i64> = if !null_bitmap.is_null() {
        let null_len = env.get_array_length(&null_bitmap).unwrap_or(0) as usize;
        if null_len > 0 {
            let mut null_buffer = vec![0i64; null_len];
            if env.get_long_array_region(&null_bitmap, 0, &mut null_buffer).is_ok() {
                null_buffer
            } else {
                vec![]
            }
        } else {
            vec![]
        }
    } else {
        vec![]
    };

    let null_set: std::collections::HashSet<usize> = null_indices.iter().map(|&x| x as usize).collect();

    let mut min: Option<f64> = None;
    for (i, &v) in buffer.iter().enumerate() {
        if !null_set.contains(&i) {
            min = Some(min.map_or(v, |m| m.min(v)));
        }
    }

    match min {
        Some(m) => {
            let double_class = env.find_class("java/lang/Double").unwrap();
            env.new_object(double_class, "(D)V", &[jni::objects::JValue::Double(m)])
                .unwrap_or(JObject::null())
        }
        None => JObject::null(),
    }
}

/// Min of long array
#[no_mangle]
pub extern "system" fn Java_org_apache_pinot_core_native_NativeAggregation_nativeMinLongArray<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
    values: JLongArray<'local>,
) -> jlong {
    let len = env.get_array_length(&values).unwrap_or(0) as usize;
    if len == 0 {
        return i64::MAX;
    }

    let mut buffer = vec![0i64; len];
    if env.get_long_array_region(&values, 0, &mut buffer).is_err() {
        return i64::MAX;
    }

    buffer.iter().cloned().min().unwrap_or(i64::MAX)
}

/// Max of double array
#[no_mangle]
pub extern "system" fn Java_org_apache_pinot_core_native_NativeAggregation_nativeMaxDoubleArray<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
    values: JDoubleArray<'local>,
) -> jdouble {
    let len = env.get_array_length(&values).unwrap_or(0) as usize;
    if len == 0 {
        return f64::MIN;
    }

    let mut buffer = vec![0.0f64; len];
    if env.get_double_array_region(&values, 0, &mut buffer).is_err() {
        return f64::MIN;
    }

    buffer.iter().cloned().fold(f64::MIN, f64::max)
}

/// Max of double array with nulls
#[no_mangle]
pub extern "system" fn Java_org_apache_pinot_core_native_NativeAggregation_nativeMaxDoubleArrayWithNulls<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
    values: JDoubleArray<'local>,
    null_bitmap: JLongArray<'local>,
) -> JObject<'local> {
    let len = env.get_array_length(&values).unwrap_or(0) as usize;
    if len == 0 {
        return JObject::null();
    }

    let mut buffer = vec![0.0f64; len];
    if env.get_double_array_region(&values, 0, &mut buffer).is_err() {
        return JObject::null();
    }

    let null_indices: Vec<i64> = if !null_bitmap.is_null() {
        let null_len = env.get_array_length(&null_bitmap).unwrap_or(0) as usize;
        if null_len > 0 {
            let mut null_buffer = vec![0i64; null_len];
            if env.get_long_array_region(&null_bitmap, 0, &mut null_buffer).is_ok() {
                null_buffer
            } else {
                vec![]
            }
        } else {
            vec![]
        }
    } else {
        vec![]
    };

    let null_set: std::collections::HashSet<usize> = null_indices.iter().map(|&x| x as usize).collect();

    let mut max: Option<f64> = None;
    for (i, &v) in buffer.iter().enumerate() {
        if !null_set.contains(&i) {
            max = Some(max.map_or(v, |m| m.max(v)));
        }
    }

    match max {
        Some(m) => {
            let double_class = env.find_class("java/lang/Double").unwrap();
            env.new_object(double_class, "(D)V", &[jni::objects::JValue::Double(m)])
                .unwrap_or(JObject::null())
        }
        None => JObject::null(),
    }
}

/// Max of long array
#[no_mangle]
pub extern "system" fn Java_org_apache_pinot_core_native_NativeAggregation_nativeMaxLongArray<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
    values: JLongArray<'local>,
) -> jlong {
    let len = env.get_array_length(&values).unwrap_or(0) as usize;
    if len == 0 {
        return i64::MIN;
    }

    let mut buffer = vec![0i64; len];
    if env.get_long_array_region(&values, 0, &mut buffer).is_err() {
        return i64::MIN;
    }

    buffer.iter().cloned().max().unwrap_or(i64::MIN)
}

/// Avg of double array - returns [sum, count]
#[no_mangle]
pub extern "system" fn Java_org_apache_pinot_core_native_NativeAggregation_nativeAvgDoubleArray<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
    values: JDoubleArray<'local>,
) -> JDoubleArray<'local> {
    let len = env.get_array_length(&values).unwrap_or(0) as usize;

    let result = env.new_double_array(2).unwrap();

    if len == 0 {
        let _ = env.set_double_array_region(&result, 0, &[0.0, 0.0]);
        return result;
    }

    let mut buffer = vec![0.0f64; len];
    if env.get_double_array_region(&values, 0, &mut buffer).is_err() {
        let _ = env.set_double_array_region(&result, 0, &[0.0, 0.0]);
        return result;
    }

    let sum: f64 = buffer.iter().sum();
    let _ = env.set_double_array_region(&result, 0, &[sum, len as f64]);
    result
}

// ==================== Int Dictionary Functions ====================

#[no_mangle]
pub extern "system" fn Java_org_apache_pinot_core_native_NativeDictionary_nativeCreateIntDictionary<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
    sorted_values: JIntArray<'local>,
) -> jlong {
    let len = env.get_array_length(&sorted_values).unwrap_or(0) as usize;
    if len == 0 {
        return 0;
    }

    let mut buffer = vec![0i32; len];
    if env.get_int_array_region(&sorted_values, 0, &mut buffer).is_err() {
        return 0;
    }

    let dict = OnHeapIntDictionary::new(buffer);
    let handle = get_next_handle();

    let mut dicts = INT_DICTIONARIES.lock().unwrap();
    dicts.insert(handle, dict);

    handle
}

#[no_mangle]
pub extern "system" fn Java_org_apache_pinot_core_native_NativeDictionary_nativeIntDictionaryIndexOf(
    _env: JNIEnv,
    _class: JClass,
    handle: jlong,
    value: jint,
) -> jint {
    let dicts = INT_DICTIONARIES.lock().unwrap();
    match dicts.get(&handle) {
        Some(dict) => dict.index_of_int(value),
        None => -1,
    }
}

#[no_mangle]
pub extern "system" fn Java_org_apache_pinot_core_native_NativeDictionary_nativeIntDictionaryGet(
    _env: JNIEnv,
    _class: JClass,
    handle: jlong,
    dict_id: jint,
) -> jint {
    let dicts = INT_DICTIONARIES.lock().unwrap();
    match dicts.get(&handle) {
        Some(dict) => dict.get_int(dict_id).unwrap_or(0),
        None => 0,
    }
}

#[no_mangle]
pub extern "system" fn Java_org_apache_pinot_core_native_NativeDictionary_nativeIntDictionaryBatchDecode<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
    handle: jlong,
    dict_ids: JIntArray<'local>,
    output: JIntArray<'local>,
) {
    let len = env.get_array_length(&dict_ids).unwrap_or(0) as usize;
    if len == 0 {
        return;
    }

    let mut ids = vec![0i32; len];
    if env.get_int_array_region(&dict_ids, 0, &mut ids).is_err() {
        return;
    }

    let dicts = INT_DICTIONARIES.lock().unwrap();
    if let Some(dict) = dicts.get(&handle) {
        let mut results = vec![0i32; len];
        for (i, &id) in ids.iter().enumerate() {
            results[i] = dict.get_int(id).unwrap_or(0);
        }
        let _ = env.set_int_array_region(&output, 0, &results);
    }
}

#[no_mangle]
pub extern "system" fn Java_org_apache_pinot_core_native_NativeDictionary_nativeIntDictionarySize(
    _env: JNIEnv,
    _class: JClass,
    handle: jlong,
) -> jint {
    let dicts = INT_DICTIONARIES.lock().unwrap();
    match dicts.get(&handle) {
        Some(dict) => dict.len() as jint,
        None => 0,
    }
}

#[no_mangle]
pub extern "system" fn Java_org_apache_pinot_core_native_NativeDictionary_nativeFreeIntDictionary(
    _env: JNIEnv,
    _class: JClass,
    handle: jlong,
) {
    let mut dicts = INT_DICTIONARIES.lock().unwrap();
    dicts.remove(&handle);
}

// ==================== Long Dictionary Functions ====================

#[no_mangle]
pub extern "system" fn Java_org_apache_pinot_core_native_NativeDictionary_nativeCreateLongDictionary<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
    sorted_values: JLongArray<'local>,
) -> jlong {
    let len = env.get_array_length(&sorted_values).unwrap_or(0) as usize;
    if len == 0 {
        return 0;
    }

    let mut buffer = vec![0i64; len];
    if env.get_long_array_region(&sorted_values, 0, &mut buffer).is_err() {
        return 0;
    }

    let dict = OnHeapLongDictionary::new(buffer);
    let handle = get_next_handle();

    let mut dicts = LONG_DICTIONARIES.lock().unwrap();
    dicts.insert(handle, dict);

    handle
}

#[no_mangle]
pub extern "system" fn Java_org_apache_pinot_core_native_NativeDictionary_nativeLongDictionaryIndexOf(
    _env: JNIEnv,
    _class: JClass,
    handle: jlong,
    value: jlong,
) -> jint {
    let dicts = LONG_DICTIONARIES.lock().unwrap();
    match dicts.get(&handle) {
        Some(dict) => dict.index_of_long(value),
        None => -1,
    }
}

#[no_mangle]
pub extern "system" fn Java_org_apache_pinot_core_native_NativeDictionary_nativeLongDictionaryGet(
    _env: JNIEnv,
    _class: JClass,
    handle: jlong,
    dict_id: jint,
) -> jlong {
    let dicts = LONG_DICTIONARIES.lock().unwrap();
    match dicts.get(&handle) {
        Some(dict) => dict.get_long(dict_id).unwrap_or(0),
        None => 0,
    }
}

#[no_mangle]
pub extern "system" fn Java_org_apache_pinot_core_native_NativeDictionary_nativeLongDictionaryBatchDecode<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
    handle: jlong,
    dict_ids: JIntArray<'local>,
    output: JLongArray<'local>,
) {
    let len = env.get_array_length(&dict_ids).unwrap_or(0) as usize;
    if len == 0 {
        return;
    }

    let mut ids = vec![0i32; len];
    if env.get_int_array_region(&dict_ids, 0, &mut ids).is_err() {
        return;
    }

    let dicts = LONG_DICTIONARIES.lock().unwrap();
    if let Some(dict) = dicts.get(&handle) {
        let mut results = vec![0i64; len];
        for (i, &id) in ids.iter().enumerate() {
            results[i] = dict.get_long(id).unwrap_or(0);
        }
        let _ = env.set_long_array_region(&output, 0, &results);
    }
}

#[no_mangle]
pub extern "system" fn Java_org_apache_pinot_core_native_NativeDictionary_nativeLongDictionarySize(
    _env: JNIEnv,
    _class: JClass,
    handle: jlong,
) -> jint {
    let dicts = LONG_DICTIONARIES.lock().unwrap();
    match dicts.get(&handle) {
        Some(dict) => dict.len() as jint,
        None => 0,
    }
}

#[no_mangle]
pub extern "system" fn Java_org_apache_pinot_core_native_NativeDictionary_nativeFreeLongDictionary(
    _env: JNIEnv,
    _class: JClass,
    handle: jlong,
) {
    let mut dicts = LONG_DICTIONARIES.lock().unwrap();
    dicts.remove(&handle);
}

// ==================== String Dictionary Functions ====================

#[no_mangle]
pub extern "system" fn Java_org_apache_pinot_core_native_NativeDictionary_nativeCreateStringDictionary<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
    sorted_values: JObjectArray<'local>,
) -> jlong {
    let len = env.get_array_length(&sorted_values).unwrap_or(0) as usize;
    if len == 0 {
        return 0;
    }

    let mut strings: Vec<String> = Vec::with_capacity(len);
    for i in 0..len {
        if let Ok(obj) = env.get_object_array_element(&sorted_values, i as i32) {
            let jstr = JString::from(obj);
            let owned_string = env.get_string(&jstr)
                .map(|s| String::from(s.to_str().unwrap_or("")))
                .unwrap_or_default();
            strings.push(owned_string);
        } else {
            strings.push(String::new());
        }
    }

    let dict = OnHeapStringDictionary::new(strings);
    let handle = get_next_handle();

    let mut dicts = STRING_DICTIONARIES.lock().unwrap();
    dicts.insert(handle, dict);

    handle
}

#[no_mangle]
pub extern "system" fn Java_org_apache_pinot_core_native_NativeDictionary_nativeStringDictionaryIndexOf<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
    handle: jlong,
    value: JString<'local>,
) -> jint {
    let search_str: String = match env.get_string(&value) {
        Ok(s) => String::from(s.to_str().unwrap_or("")),
        Err(_) => return -1,
    };

    let dicts = STRING_DICTIONARIES.lock().unwrap();
    match dicts.get(&handle) {
        Some(dict) => dict.index_of_string(&search_str),
        None => -1,
    }
}

#[no_mangle]
pub extern "system" fn Java_org_apache_pinot_core_native_NativeDictionary_nativeStringDictionaryGet<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
    handle: jlong,
    dict_id: jint,
) -> JString<'local> {
    let dicts = STRING_DICTIONARIES.lock().unwrap();
    match dicts.get(&handle) {
        Some(dict) => {
            match dict.get_string(dict_id) {
                Ok(v) => env.new_string(&v).unwrap_or_else(|_| env.new_string("").unwrap()),
                Err(_) => env.new_string("").unwrap(),
            }
        }
        None => env.new_string("").unwrap(),
    }
}

#[no_mangle]
pub extern "system" fn Java_org_apache_pinot_core_native_NativeDictionary_nativeStringDictionarySize(
    _env: JNIEnv,
    _class: JClass,
    handle: jlong,
) -> jint {
    let dicts = STRING_DICTIONARIES.lock().unwrap();
    match dicts.get(&handle) {
        Some(dict) => dict.len() as jint,
        None => 0,
    }
}

#[no_mangle]
pub extern "system" fn Java_org_apache_pinot_core_native_NativeDictionary_nativeFreeStringDictionary(
    _env: JNIEnv,
    _class: JClass,
    handle: jlong,
) {
    let mut dicts = STRING_DICTIONARIES.lock().unwrap();
    dicts.remove(&handle);
}

// ==================== Bitmap Functions ====================

fn read_byte_array(env: &mut JNIEnv, array: &JByteArray) -> Option<Vec<u8>> {
    let len = env.get_array_length(array).ok()? as usize;
    if len == 0 {
        return None;
    }

    let mut i8_buf = vec![0i8; len];
    env.get_byte_array_region(array, 0, &mut i8_buf).ok()?;

    Some(i8_buf.iter().map(|&b| b as u8).collect())
}

fn write_byte_array<'local>(env: &mut JNIEnv<'local>, data: &[u8]) -> JByteArray<'local> {
    let result = env.new_byte_array(data.len() as i32).unwrap();
    let i8_data: Vec<i8> = data.iter().map(|&b| b as i8).collect();
    let _ = env.set_byte_array_region(&result, 0, &i8_data);
    result
}

#[no_mangle]
pub extern "system" fn Java_org_apache_pinot_core_native_NativeBitmap_nativeAndCardinality<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
    bitmap1: JByteArray<'local>,
    bitmap2: JByteArray<'local>,
) -> jlong {
    let buf1 = match read_byte_array(&mut env, &bitmap1) {
        Some(b) => b,
        None => return 0,
    };
    let buf2 = match read_byte_array(&mut env, &bitmap2) {
        Some(b) => b,
        None => return 0,
    };

    use roaring::RoaringBitmap;
    let bm1 = match RoaringBitmap::deserialize_from(&buf1[..]) {
        Ok(bm) => bm,
        Err(_) => return 0,
    };
    let bm2 = match RoaringBitmap::deserialize_from(&buf2[..]) {
        Ok(bm) => bm,
        Err(_) => return 0,
    };

    (&bm1 & &bm2).len() as jlong
}

#[no_mangle]
pub extern "system" fn Java_org_apache_pinot_core_native_NativeBitmap_nativeOrCardinality<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
    bitmap1: JByteArray<'local>,
    bitmap2: JByteArray<'local>,
) -> jlong {
    let buf1 = match read_byte_array(&mut env, &bitmap1) {
        Some(b) => b,
        None => return 0,
    };
    let buf2 = match read_byte_array(&mut env, &bitmap2) {
        Some(b) => b,
        None => return 0,
    };

    use roaring::RoaringBitmap;
    let bm1 = match RoaringBitmap::deserialize_from(&buf1[..]) {
        Ok(bm) => bm,
        Err(_) => return 0,
    };
    let bm2 = match RoaringBitmap::deserialize_from(&buf2[..]) {
        Ok(bm) => bm,
        Err(_) => return 0,
    };

    (&bm1 | &bm2).len() as jlong
}

#[no_mangle]
pub extern "system" fn Java_org_apache_pinot_core_native_NativeBitmap_nativeAndNotCardinality<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
    bitmap1: JByteArray<'local>,
    bitmap2: JByteArray<'local>,
) -> jlong {
    let buf1 = match read_byte_array(&mut env, &bitmap1) {
        Some(b) => b,
        None => return 0,
    };

    use roaring::RoaringBitmap;
    let bm1 = match RoaringBitmap::deserialize_from(&buf1[..]) {
        Ok(bm) => bm,
        Err(_) => return 0,
    };

    let buf2 = match read_byte_array(&mut env, &bitmap2) {
        Some(b) => b,
        None => return bm1.len() as jlong,
    };

    let bm2 = match RoaringBitmap::deserialize_from(&buf2[..]) {
        Ok(bm) => bm,
        Err(_) => return bm1.len() as jlong,
    };

    (&bm1 - &bm2).len() as jlong
}

#[no_mangle]
pub extern "system" fn Java_org_apache_pinot_core_native_NativeBitmap_nativeXorCardinality<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
    bitmap1: JByteArray<'local>,
    bitmap2: JByteArray<'local>,
) -> jlong {
    let buf1 = match read_byte_array(&mut env, &bitmap1) {
        Some(b) => b,
        None => return 0,
    };
    let buf2 = match read_byte_array(&mut env, &bitmap2) {
        Some(b) => b,
        None => return 0,
    };

    use roaring::RoaringBitmap;
    let bm1 = match RoaringBitmap::deserialize_from(&buf1[..]) {
        Ok(bm) => bm,
        Err(_) => return 0,
    };
    let bm2 = match RoaringBitmap::deserialize_from(&buf2[..]) {
        Ok(bm) => bm,
        Err(_) => return 0,
    };

    (&bm1 ^ &bm2).len() as jlong
}

#[no_mangle]
pub extern "system" fn Java_org_apache_pinot_core_native_NativeBitmap_nativeAnd<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
    bitmap1: JByteArray<'local>,
    bitmap2: JByteArray<'local>,
) -> JByteArray<'local> {
    let empty = env.new_byte_array(0).unwrap();

    let buf1 = match read_byte_array(&mut env, &bitmap1) {
        Some(b) => b,
        None => return empty,
    };
    let buf2 = match read_byte_array(&mut env, &bitmap2) {
        Some(b) => b,
        None => return empty,
    };

    use roaring::RoaringBitmap;
    let bm1 = match RoaringBitmap::deserialize_from(&buf1[..]) {
        Ok(bm) => bm,
        Err(_) => return empty,
    };
    let bm2 = match RoaringBitmap::deserialize_from(&buf2[..]) {
        Ok(bm) => bm,
        Err(_) => return empty,
    };

    let result = &bm1 & &bm2;
    let mut serialized = Vec::new();
    if result.serialize_into(&mut serialized).is_err() {
        return empty;
    }

    write_byte_array(&mut env, &serialized)
}

#[no_mangle]
pub extern "system" fn Java_org_apache_pinot_core_native_NativeBitmap_nativeOr<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
    bitmap1: JByteArray<'local>,
    bitmap2: JByteArray<'local>,
) -> JByteArray<'local> {
    let empty = env.new_byte_array(0).unwrap();

    let buf1 = match read_byte_array(&mut env, &bitmap1) {
        Some(b) => b,
        None => return empty,
    };
    let buf2 = match read_byte_array(&mut env, &bitmap2) {
        Some(b) => b,
        None => return empty,
    };

    use roaring::RoaringBitmap;
    let bm1 = match RoaringBitmap::deserialize_from(&buf1[..]) {
        Ok(bm) => bm,
        Err(_) => return empty,
    };
    let bm2 = match RoaringBitmap::deserialize_from(&buf2[..]) {
        Ok(bm) => bm,
        Err(_) => return empty,
    };

    let result = &bm1 | &bm2;
    let mut serialized = Vec::new();
    if result.serialize_into(&mut serialized).is_err() {
        return empty;
    }

    write_byte_array(&mut env, &serialized)
}

#[no_mangle]
pub extern "system" fn Java_org_apache_pinot_core_native_NativeBitmap_nativeAndNot<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
    bitmap1: JByteArray<'local>,
    bitmap2: JByteArray<'local>,
) -> JByteArray<'local> {
    let empty = env.new_byte_array(0).unwrap();

    let buf1 = match read_byte_array(&mut env, &bitmap1) {
        Some(b) => b,
        None => return empty,
    };

    use roaring::RoaringBitmap;
    let bm1 = match RoaringBitmap::deserialize_from(&buf1[..]) {
        Ok(bm) => bm,
        Err(_) => return empty,
    };

    let buf2 = match read_byte_array(&mut env, &bitmap2) {
        Some(b) => b,
        None => {
            // Return bm1 as-is
            let mut serialized = Vec::new();
            if bm1.serialize_into(&mut serialized).is_err() {
                return empty;
            }
            return write_byte_array(&mut env, &serialized);
        }
    };

    let bm2 = match RoaringBitmap::deserialize_from(&buf2[..]) {
        Ok(bm) => bm,
        Err(_) => return empty,
    };

    let result = &bm1 - &bm2;
    let mut serialized = Vec::new();
    if result.serialize_into(&mut serialized).is_err() {
        return empty;
    }

    write_byte_array(&mut env, &serialized)
}

#[cfg(test)]
mod tests {
    #[test]
    fn test_jni_compilation() {
        // This test just verifies the JNI module compiles
        assert!(true);
    }
}
