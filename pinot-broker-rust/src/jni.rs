//! JNI bindings for Java integration.

use crate::config::{BrokerConfig, RoutingConfig};
use crate::reduce::{BrokerReduceService, ReduceConfig};
use crate::routing::RoutingManager;
use crate::transport::query_router::{QueryRouter, QueryRouterConfig};
use crate::types::*;
use crate::{BrokerError, Result};
use jni::objects::{JClass, JObject, JString, JValue};
use jni::sys::{jboolean, jint, jlong};
use jni::JNIEnv;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

// Handle storage for native objects
static mut ROUTING_MANAGERS: Option<HashMap<jlong, Arc<RoutingManager>>> = None;
static mut QUERY_ROUTERS: Option<HashMap<jlong, Arc<QueryRouter>>> = None;
static mut REDUCE_SERVICES: Option<HashMap<jlong, Arc<BrokerReduceService>>> = None;
static HANDLE_COUNTER: std::sync::atomic::AtomicI64 = std::sync::atomic::AtomicI64::new(1);

fn init_storage() {
    unsafe {
        if ROUTING_MANAGERS.is_none() {
            ROUTING_MANAGERS = Some(HashMap::new());
        }
        if QUERY_ROUTERS.is_none() {
            QUERY_ROUTERS = Some(HashMap::new());
        }
        if REDUCE_SERVICES.is_none() {
            REDUCE_SERVICES = Some(HashMap::new());
        }
    }
}

fn next_handle() -> jlong {
    HANDLE_COUNTER.fetch_add(1, std::sync::atomic::Ordering::SeqCst)
}

// ==================== RoutingManager JNI ====================

/// Create a new RoutingManager.
#[no_mangle]
pub extern "system" fn Java_org_apache_pinot_broker_routing_NativeRoutingManager_create<'local>(
    mut _env: JNIEnv<'local>,
    _class: JClass<'local>,
) -> jlong {
    init_storage();
    let config = RoutingConfig::default();
    let manager = Arc::new(RoutingManager::new(config));
    let handle = next_handle();

    unsafe {
        ROUTING_MANAGERS.as_mut().unwrap().insert(handle, manager);
    }

    handle
}

/// Destroy a RoutingManager.
#[no_mangle]
pub extern "system" fn Java_org_apache_pinot_broker_routing_NativeRoutingManager_destroy<'local>(
    mut _env: JNIEnv<'local>,
    _class: JClass<'local>,
    handle: jlong,
) {
    unsafe {
        if let Some(managers) = ROUTING_MANAGERS.as_mut() {
            managers.remove(&handle);
        }
    }
}

/// Register a table.
#[no_mangle]
pub extern "system" fn Java_org_apache_pinot_broker_routing_NativeRoutingManager_registerTable<
    'local,
>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
    handle: jlong,
    table_name: JString<'local>,
) {
    let table: String = env
        .get_string(&table_name)
        .map(|s| s.into())
        .unwrap_or_default();

    unsafe {
        if let Some(managers) = ROUTING_MANAGERS.as_ref() {
            if let Some(manager) = managers.get(&handle) {
                manager.register_table(table);
            }
        }
    }
}

/// Unregister a table.
#[no_mangle]
pub extern "system" fn Java_org_apache_pinot_broker_routing_NativeRoutingManager_unregisterTable<
    'local,
>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
    handle: jlong,
    table_name: JString<'local>,
) {
    let table: String = env
        .get_string(&table_name)
        .map(|s| s.into())
        .unwrap_or_default();

    unsafe {
        if let Some(managers) = ROUTING_MANAGERS.as_ref() {
            if let Some(manager) = managers.get(&handle) {
                manager.unregister_table(&table);
            }
        }
    }
}

/// Check if table exists.
#[no_mangle]
pub extern "system" fn Java_org_apache_pinot_broker_routing_NativeRoutingManager_hasTable<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
    handle: jlong,
    table_name: JString<'local>,
) -> jboolean {
    let table: String = env
        .get_string(&table_name)
        .map(|s| s.into())
        .unwrap_or_default();

    unsafe {
        if let Some(managers) = ROUTING_MANAGERS.as_ref() {
            if let Some(manager) = managers.get(&handle) {
                return if manager.has_table(&table) { 1 } else { 0 };
            }
        }
    }
    0
}

/// Get number of tables.
#[no_mangle]
pub extern "system" fn Java_org_apache_pinot_broker_routing_NativeRoutingManager_getNumTables<
    'local,
>(
    _env: JNIEnv<'local>,
    _class: JClass<'local>,
    handle: jlong,
) -> jint {
    unsafe {
        if let Some(managers) = ROUTING_MANAGERS.as_ref() {
            if let Some(manager) = managers.get(&handle) {
                return manager.get_stats().num_tables as jint;
            }
        }
    }
    0
}

// ==================== QueryRouter JNI ====================

/// Create a new QueryRouter.
#[no_mangle]
pub extern "system" fn Java_org_apache_pinot_broker_transport_NativeQueryRouter_create<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
    broker_id: JString<'local>,
    timeout_ms: jlong,
    max_concurrent: jint,
) -> jlong {
    init_storage();

    let broker: String = env
        .get_string(&broker_id)
        .map(|s| s.into())
        .unwrap_or_else(|_| "broker".to_string());

    let config = QueryRouterConfig {
        query_timeout: Duration::from_millis(timeout_ms as u64),
        max_concurrent_queries: max_concurrent as usize,
        ..Default::default()
    };

    let router = Arc::new(QueryRouter::new(broker, config));
    let handle = next_handle();

    unsafe {
        QUERY_ROUTERS.as_mut().unwrap().insert(handle, router);
    }

    handle
}

/// Destroy a QueryRouter.
#[no_mangle]
pub extern "system" fn Java_org_apache_pinot_broker_transport_NativeQueryRouter_destroy<'local>(
    _env: JNIEnv<'local>,
    _class: JClass<'local>,
    handle: jlong,
) {
    unsafe {
        if let Some(routers) = QUERY_ROUTERS.as_mut() {
            routers.remove(&handle);
        }
    }
}

/// Get router statistics.
#[no_mangle]
pub extern "system" fn Java_org_apache_pinot_broker_transport_NativeQueryRouter_getNumChannels<
    'local,
>(
    _env: JNIEnv<'local>,
    _class: JClass<'local>,
    handle: jlong,
) -> jint {
    unsafe {
        if let Some(routers) = QUERY_ROUTERS.as_ref() {
            if let Some(router) = routers.get(&handle) {
                return router.stats().num_channels as jint;
            }
        }
    }
    0
}

/// Get available permits.
#[no_mangle]
pub extern "system" fn Java_org_apache_pinot_broker_transport_NativeQueryRouter_getAvailablePermits<
    'local,
>(
    _env: JNIEnv<'local>,
    _class: JClass<'local>,
    handle: jlong,
) -> jint {
    unsafe {
        if let Some(routers) = QUERY_ROUTERS.as_ref() {
            if let Some(router) = routers.get(&handle) {
                return router.stats().available_permits as jint;
            }
        }
    }
    0
}

// ==================== BrokerReduceService JNI ====================

/// Create a new BrokerReduceService.
#[no_mangle]
pub extern "system" fn Java_org_apache_pinot_broker_reduce_NativeBrokerReduceService_create<
    'local,
>(
    _env: JNIEnv<'local>,
    _class: JClass<'local>,
    max_rows: jint,
    enable_parallel: jboolean,
) -> jlong {
    init_storage();

    let config = ReduceConfig {
        max_rows: max_rows as usize,
        enable_parallel: enable_parallel != 0,
        ..Default::default()
    };

    let service = Arc::new(BrokerReduceService::new(config));
    let handle = next_handle();

    unsafe {
        REDUCE_SERVICES.as_mut().unwrap().insert(handle, service);
    }

    handle
}

/// Destroy a BrokerReduceService.
#[no_mangle]
pub extern "system" fn Java_org_apache_pinot_broker_reduce_NativeBrokerReduceService_destroy<
    'local,
>(
    _env: JNIEnv<'local>,
    _class: JClass<'local>,
    handle: jlong,
) {
    unsafe {
        if let Some(services) = REDUCE_SERVICES.as_mut() {
            services.remove(&handle);
        }
    }
}

// ==================== Utility Functions ====================

/// Get library version.
#[no_mangle]
pub extern "system" fn Java_org_apache_pinot_broker_native_NativeBroker_getVersion<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
) -> JString<'local> {
    env.new_string(env!("CARGO_PKG_VERSION"))
        .unwrap_or_else(|_| env.new_string("unknown").unwrap())
}

/// Check if native library is loaded.
#[no_mangle]
pub extern "system" fn Java_org_apache_pinot_broker_native_NativeBroker_isLoaded<'local>(
    _env: JNIEnv<'local>,
    _class: JClass<'local>,
) -> jboolean {
    1
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_handle_counter() {
        let h1 = next_handle();
        let h2 = next_handle();
        assert!(h2 > h1);
    }

    #[test]
    fn test_init_storage() {
        init_storage();
        unsafe {
            assert!(ROUTING_MANAGERS.is_some());
            assert!(QUERY_ROUTERS.is_some());
            assert!(REDUCE_SERVICES.is_some());
        }
    }
}
