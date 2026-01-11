//! JNI bindings for Java integration
//!
//! This module provides JNI-compatible functions for calling the Rust query server
//! from Java code.

#![allow(unused_mut)]

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use jni::objects::{JByteArray, JClass, JObject, JObjectArray, JString, JValue};
use jni::sys::{jboolean, jdouble, jint, jlong};
use jni::JNIEnv;
use parking_lot::Mutex;

use crate::backpressure::{MemoryBackpressureManager, MemoryPressureLevel};
use crate::block::{BlockId, BlockSchema, ColumnData, ColumnSchema, ColumnType, DataBlock, MseBlock};
use crate::config::{MemoryConfig, QueryServerConfig};
use crate::mailbox::{MailboxId, MailboxService, ReceivingMailbox};
use crate::scheduler::{QueryInfo, QueryPriority, QueryScheduler, QueryState};

// Global state
lazy_static::lazy_static! {
    static ref MAILBOX_SERVICE: Mutex<Option<Arc<MailboxService>>> = Mutex::new(None);
    static ref SCHEDULER: Mutex<Option<Arc<QueryScheduler>>> = Mutex::new(None);
    static ref BACKPRESSURE: Mutex<Option<Arc<MemoryBackpressureManager>>> = Mutex::new(None);
    static ref RECEIVING_MAILBOXES: Mutex<HashMap<jlong, Arc<ReceivingMailbox>>> = Mutex::new(HashMap::new());
    static ref NEXT_HANDLE: Mutex<jlong> = Mutex::new(1);
}

fn get_next_handle() -> jlong {
    let mut handle = NEXT_HANDLE.lock();
    let current = *handle;
    *handle += 1;
    current
}

// ==================== Initialization ====================

/// Initialize the Rust query server
#[no_mangle]
pub extern "system" fn Java_org_apache_pinot_query_runtime_native_NativeQueryServer_nativeInit<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
    max_memory_bytes: jlong,
    max_pending_blocks: jint,
) -> jboolean {
    // Create configuration
    let mut config = QueryServerConfig::default();
    if max_pending_blocks > 0 {
        config.max_pending_blocks = max_pending_blocks as usize;
    }
    if max_memory_bytes > 0 {
        config.memory.max_memory_bytes = max_memory_bytes as usize;
    }

    let config = Arc::new(config);

    // Create backpressure manager
    let bp_config = MemoryConfig {
        max_memory_bytes: config.memory.max_memory_bytes,
        ..Default::default()
    };
    let backpressure = Arc::new(MemoryBackpressureManager::new(bp_config));

    // Create mailbox service
    let mailbox_service = Arc::new(MailboxService::new(config.clone()));

    // Create scheduler
    let scheduler = Arc::new(QueryScheduler::new(config, backpressure.clone()));

    // Store in globals
    *MAILBOX_SERVICE.lock() = Some(mailbox_service);
    *SCHEDULER.lock() = Some(scheduler);
    *BACKPRESSURE.lock() = Some(backpressure);

    1 // true
}

/// Shutdown the Rust query server
#[no_mangle]
pub extern "system" fn Java_org_apache_pinot_query_runtime_native_NativeQueryServer_nativeShutdown<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
) {
    if let Some(scheduler) = SCHEDULER.lock().take() {
        scheduler.shutdown();
    }
    if let Some(mailbox_service) = MAILBOX_SERVICE.lock().take() {
        mailbox_service.shutdown();
    }
    if let Some(backpressure) = BACKPRESSURE.lock().take() {
        backpressure.shutdown();
    }
    RECEIVING_MAILBOXES.lock().clear();
}

// ==================== Memory Backpressure ====================

/// Get current memory pressure level
#[no_mangle]
pub extern "system" fn Java_org_apache_pinot_query_runtime_native_NativeQueryServer_nativeGetMemoryPressureLevel<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
) -> jint {
    if let Some(bp) = BACKPRESSURE.lock().as_ref() {
        match bp.pressure_level() {
            MemoryPressureLevel::Normal => 0,
            MemoryPressureLevel::Moderate => 1,
            MemoryPressureLevel::High => 2,
            MemoryPressureLevel::Critical => 3,
        }
    } else {
        0
    }
}

/// Get current memory usage in bytes
#[no_mangle]
pub extern "system" fn Java_org_apache_pinot_query_runtime_native_NativeQueryServer_nativeGetMemoryUsed<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
) -> jlong {
    if let Some(bp) = BACKPRESSURE.lock().as_ref() {
        bp.memory_stats().allocated_bytes as jlong
    } else {
        0
    }
}

/// Get reserved memory in bytes
#[no_mangle]
pub extern "system" fn Java_org_apache_pinot_query_runtime_native_NativeQueryServer_nativeGetReservedMemory<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
) -> jlong {
    if let Some(bp) = BACKPRESSURE.lock().as_ref() {
        bp.reserved_bytes() as jlong
    } else {
        0
    }
}

/// Check if should throttle due to memory pressure
#[no_mangle]
pub extern "system" fn Java_org_apache_pinot_query_runtime_native_NativeQueryServer_nativeShouldThrottle<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
) -> jboolean {
    if let Some(bp) = BACKPRESSURE.lock().as_ref() {
        if bp.should_throttle() { 1 } else { 0 }
    } else {
        0
    }
}

/// Get number of active queries
#[no_mangle]
pub extern "system" fn Java_org_apache_pinot_query_runtime_native_NativeQueryServer_nativeGetActiveQueries<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
) -> jint {
    if let Some(bp) = BACKPRESSURE.lock().as_ref() {
        bp.active_queries() as jint
    } else {
        0
    }
}

// ==================== Mailbox Operations ====================

/// Create a receiving mailbox
#[no_mangle]
pub extern "system" fn Java_org_apache_pinot_query_runtime_native_NativeQueryServer_nativeCreateReceivingMailbox<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
    query_id: JString<'local>,
    sender_stage_id: jint,
    sender_worker_id: jint,
    receiver_stage_id: jint,
    receiver_worker_id: jint,
    max_pending_blocks: jint,
) -> jlong {
    let query_id_str: String = match env.get_string(&query_id) {
        Ok(s) => String::from(s.to_str().unwrap_or("")),
        Err(_) => return 0,
    };

    let mailbox_id = MailboxId::new(
        query_id_str,
        sender_stage_id,
        sender_worker_id,
        receiver_stage_id,
        receiver_worker_id,
    );

    let max_blocks = if max_pending_blocks > 0 {
        max_pending_blocks as usize
    } else {
        5
    };

    let mailbox = Arc::new(ReceivingMailbox::new(mailbox_id, max_blocks));
    let handle = get_next_handle();

    RECEIVING_MAILBOXES.lock().insert(handle, mailbox);

    handle
}

/// Close a receiving mailbox
#[no_mangle]
pub extern "system" fn Java_org_apache_pinot_query_runtime_native_NativeQueryServer_nativeCloseReceivingMailbox<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
    handle: jlong,
) {
    if let Some(mailbox) = RECEIVING_MAILBOXES.lock().remove(&handle) {
        mailbox.close();
    }
}

/// Get number of pending blocks in mailbox
#[no_mangle]
pub extern "system" fn Java_org_apache_pinot_query_runtime_native_NativeQueryServer_nativeGetPendingBlocks<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
    handle: jlong,
) -> jint {
    if let Some(mailbox) = RECEIVING_MAILBOXES.lock().get(&handle) {
        mailbox.pending_blocks() as jint
    } else {
        -1
    }
}

/// Check if mailbox is full
#[no_mangle]
pub extern "system" fn Java_org_apache_pinot_query_runtime_native_NativeQueryServer_nativeIsMailboxFull<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
    handle: jlong,
) -> jboolean {
    if let Some(mailbox) = RECEIVING_MAILBOXES.lock().get(&handle) {
        if mailbox.is_full() { 1 } else { 0 }
    } else {
        0
    }
}

/// Signal early termination on mailbox
#[no_mangle]
pub extern "system" fn Java_org_apache_pinot_query_runtime_native_NativeQueryServer_nativeSignalEarlyTermination<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
    handle: jlong,
) {
    if let Some(mailbox) = RECEIVING_MAILBOXES.lock().get(&handle) {
        mailbox.signal_early_termination();
    }
}

// ==================== Scheduler Operations ====================

/// Get number of running queries
#[no_mangle]
pub extern "system" fn Java_org_apache_pinot_query_runtime_native_NativeQueryServer_nativeGetRunningQueries<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
) -> jint {
    if let Some(scheduler) = SCHEDULER.lock().as_ref() {
        scheduler.running_count() as jint
    } else {
        0
    }
}

/// Get number of queued queries
#[no_mangle]
pub extern "system" fn Java_org_apache_pinot_query_runtime_native_NativeQueryServer_nativeGetQueuedQueries<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
) -> jint {
    if let Some(scheduler) = SCHEDULER.lock().as_ref() {
        scheduler.queued_count() as jint
    } else {
        0
    }
}

/// Cancel a query
#[no_mangle]
pub extern "system" fn Java_org_apache_pinot_query_runtime_native_NativeQueryServer_nativeCancelQuery<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
    query_id: JString<'local>,
) -> jboolean {
    let query_id_str: String = match env.get_string(&query_id) {
        Ok(s) => String::from(s.to_str().unwrap_or("")),
        Err(_) => return 0,
    };

    if let Some(scheduler) = SCHEDULER.lock().as_ref() {
        if scheduler.cancel_query(&query_id_str) { 1 } else { 0 }
    } else {
        0
    }
}

// ==================== Statistics ====================

/// Get total queries submitted
#[no_mangle]
pub extern "system" fn Java_org_apache_pinot_query_runtime_native_NativeQueryServer_nativeGetTotalQueries<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
) -> jlong {
    if let Some(scheduler) = SCHEDULER.lock().as_ref() {
        scheduler.stats().queries_submitted.load(std::sync::atomic::Ordering::Relaxed) as jlong
    } else {
        0
    }
}

/// Get total queries completed
#[no_mangle]
pub extern "system" fn Java_org_apache_pinot_query_runtime_native_NativeQueryServer_nativeGetCompletedQueries<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
) -> jlong {
    if let Some(scheduler) = SCHEDULER.lock().as_ref() {
        scheduler.stats().queries_completed.load(std::sync::atomic::Ordering::Relaxed) as jlong
    } else {
        0
    }
}

/// Get total queries failed
#[no_mangle]
pub extern "system" fn Java_org_apache_pinot_query_runtime_native_NativeQueryServer_nativeGetFailedQueries<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
) -> jlong {
    if let Some(scheduler) = SCHEDULER.lock().as_ref() {
        scheduler.stats().queries_failed.load(std::sync::atomic::Ordering::Relaxed) as jlong
    } else {
        0
    }
}

/// Get average execution time in milliseconds
#[no_mangle]
pub extern "system" fn Java_org_apache_pinot_query_runtime_native_NativeQueryServer_nativeGetAverageExecutionTimeMs<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
) -> jlong {
    if let Some(scheduler) = SCHEDULER.lock().as_ref() {
        scheduler.stats().average_execution_time().as_millis() as jlong
    } else {
        0
    }
}

/// Get average queue wait time in milliseconds
#[no_mangle]
pub extern "system" fn Java_org_apache_pinot_query_runtime_native_NativeQueryServer_nativeGetAverageQueueWaitMs<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
) -> jlong {
    if let Some(scheduler) = SCHEDULER.lock().as_ref() {
        scheduler.stats().average_queue_wait().as_millis() as jlong
    } else {
        0
    }
}

/// Get backpressure event count
#[no_mangle]
pub extern "system" fn Java_org_apache_pinot_query_runtime_native_NativeQueryServer_nativeGetBackpressureEvents<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
) -> jlong {
    if let Some(bp) = BACKPRESSURE.lock().as_ref() {
        bp.stats().queries_throttled.load(std::sync::atomic::Ordering::Relaxed) as jlong
    } else {
        0
    }
}

/// Get queries rejected due to memory pressure
#[no_mangle]
pub extern "system" fn Java_org_apache_pinot_query_runtime_native_NativeQueryServer_nativeGetRejectedQueries<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
) -> jlong {
    if let Some(bp) = BACKPRESSURE.lock().as_ref() {
        bp.stats().queries_rejected.load(std::sync::atomic::Ordering::Relaxed) as jlong
    } else {
        0
    }
}

/// Check if native query server is initialized
#[no_mangle]
pub extern "system" fn Java_org_apache_pinot_query_runtime_native_NativeQueryServer_nativeIsInitialized<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
) -> jboolean {
    let has_service = MAILBOX_SERVICE.lock().is_some();
    let has_scheduler = SCHEDULER.lock().is_some();
    let has_bp = BACKPRESSURE.lock().is_some();

    if has_service && has_scheduler && has_bp { 1 } else { 0 }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_handle_generation() {
        let h1 = get_next_handle();
        let h2 = get_next_handle();
        assert!(h2 > h1);
    }
}
