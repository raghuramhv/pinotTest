//! Integration tests for Pinot Broker Rust components
//!
//! These tests verify the end-to-end functionality of the broker,
//! including routing, reduce, and transport layers working together.

use pinot_broker::access_control::{
    AccessControlManager, Permission, Principal, RowFilter, TableAccessPolicy,
};
use pinot_broker::cancellation::{CancellationToken, QueryManager, QueryState};
use pinot_broker::config::RoutingConfig;
use pinot_broker::quota::{QuotaConfig, QuotaManager};
use pinot_broker::reduce::{BrokerReduceService, ReduceConfig};
use pinot_broker::routing::RoutingManagerBuilder;
use pinot_broker::types::*;
use pinot_broker::BrokerError;
use std::sync::Arc;
use std::time::Duration;

// ============== Helper Functions ==============

fn create_server(name: &str, port: u16) -> ServerInstance {
    ServerInstance::new(name.to_string(), port, TableType::Offline)
}

fn create_schema() -> DataSchema {
    DataSchema::new(
        vec!["id".to_string(), "value".to_string()],
        vec![ColumnDataType::Long, ColumnDataType::Double],
    )
}

fn create_data_table(schema: DataSchema, rows: Vec<Vec<DataValue>>) -> DataTable {
    let mut dt = DataTable::new(schema);
    for row in rows {
        dt.add_row(row);
    }
    dt
}

fn create_server_response(
    server: ServerInstance,
    rows: Vec<Vec<DataValue>>,
) -> (ServerInstance, ServerResponse) {
    let schema = create_schema();
    let dt = create_data_table(schema, rows);
    (server.clone(), ServerResponse::success(server, dt, 100))
}

// ============== End-to-End Routing Tests ==============

#[test]
fn test_full_routing_pipeline() {
    // Create routing manager
    let config = RoutingConfig::default();
    let manager = RoutingManagerBuilder::new().config(config).build();

    // Register table with segments
    manager.register_table("test_table".to_string());

    let servers: Vec<ServerInstance> = (0..5)
        .map(|i| create_server(&format!("server{}", i), 8099 + i as u16))
        .collect();

    // Add segment mappings
    for i in 0..20 {
        let segment = format!("segment_{}", i);
        let server1 = servers[i % servers.len()].clone();
        let server2 = servers[(i + 1) % servers.len()].clone();
        manager.update_segment_mapping("test_table", segment, vec![server1, server2]);
    }

    // Create a request and get routing
    let request = BrokerRequest::new(
        1,
        "SELECT * FROM test_table".to_string(),
        "test_table".to_string(),
    );

    let routing = manager.get_routing_table(&request, 1);
    assert!(routing.is_ok());

    let table = routing.unwrap();
    assert!(!table.is_empty());
    assert_eq!(table.num_segments(), 20);
}

#[test]
fn test_routing_with_single_server() {
    let manager = RoutingManagerBuilder::new().build();

    manager.register_table("single_table".to_string());
    let server = create_server("server0", 8099);
    manager.update_segment_mapping("single_table", "segment_0".to_string(), vec![server]);

    let request = BrokerRequest::new(
        1,
        "SELECT * FROM single_table".to_string(),
        "single_table".to_string(),
    );
    let routing = manager.get_routing_table(&request, 1).unwrap();

    assert_eq!(routing.num_servers(), 1);
    assert_eq!(routing.num_segments(), 1);
}

#[test]
fn test_routing_table_not_found() {
    let manager = RoutingManagerBuilder::new().build();

    let request = BrokerRequest::new(
        1,
        "SELECT * FROM nonexistent".to_string(),
        "nonexistent".to_string(),
    );
    let result = manager.get_routing_table(&request, 1);

    assert!(matches!(result, Err(BrokerError::TableNotFound(_))));
}

// ============== End-to-End Reduce Tests ==============

#[test]
fn test_full_reduce_pipeline() {
    let config = ReduceConfig::default();
    let service = BrokerReduceService::new(config);

    // Create mock server responses
    let responses: Vec<(ServerInstance, ServerResponse)> = (0..5)
        .map(|i| {
            let rows: Vec<Vec<DataValue>> = (0..100)
                .map(|j| {
                    vec![
                        DataValue::Long((i * 100 + j) as i64),
                        DataValue::Double((i * 100 + j) as f64 * 1.5),
                    ]
                })
                .collect();
            create_server_response(create_server(&format!("server{}", i), 8099), rows)
        })
        .collect();

    // Reduce responses
    let result = service.reduce(responses, 5);
    assert!(result.is_ok());

    let broker_response = result.unwrap();
    assert!(broker_response.result_table.is_some());
    assert_eq!(broker_response.result_table.unwrap().num_rows(), 500); // 5 servers * 100 rows
    assert!(broker_response.exceptions.is_empty());
}

#[test]
fn test_reduce_with_limit() {
    let config = ReduceConfig {
        max_rows: 50,
        ..Default::default()
    };
    let service = BrokerReduceService::new(config);

    let responses: Vec<(ServerInstance, ServerResponse)> = (0..3)
        .map(|i| {
            let rows: Vec<Vec<DataValue>> = (0..100)
                .map(|j| {
                    vec![
                        DataValue::Long((i * 100 + j) as i64),
                        DataValue::Double((i * 100 + j) as f64),
                    ]
                })
                .collect();
            create_server_response(create_server(&format!("server{}", i), 8099), rows)
        })
        .collect();

    let result = service.reduce(responses, 3);
    assert!(result.is_ok());

    let broker_response = result.unwrap();
    assert!(broker_response.result_table.is_some());
    assert_eq!(broker_response.result_table.unwrap().num_rows(), 50);
}

#[test]
fn test_reduce_with_failed_server() {
    let service = BrokerReduceService::default();

    let rows = vec![vec![DataValue::Long(1), DataValue::Double(1.0)]];

    let mut responses: Vec<(ServerInstance, ServerResponse)> =
        vec![create_server_response(create_server("server0", 8099), rows)];

    // Add a failed response
    let failed_server = create_server("server1", 8100);
    responses.push((
        failed_server.clone(),
        ServerResponse::error(failed_server, BrokerError::Timeout(1000)),
    ));

    let result = service.reduce(responses, 2);
    assert!(result.is_ok());

    let broker_response = result.unwrap();
    assert_eq!(broker_response.num_servers_queried, 2);
    assert_eq!(broker_response.num_servers_responded, 1);
    assert_eq!(broker_response.exceptions.len(), 1);
}

#[test]
fn test_reduce_empty_responses() {
    let service = BrokerReduceService::default();
    let result = service.reduce(vec![], 0).unwrap();

    assert_eq!(result.num_servers_queried, 0);
    assert_eq!(result.num_servers_responded, 0);
    assert!(result.result_table.is_none());
}

// ============== Access Control Integration Tests ==============

#[test]
fn test_access_control_full_flow() {
    // Create access control manager
    let acl = AccessControlManager::new(true);

    // Create policy with RLS and CLS using builder pattern
    let policy = TableAccessPolicy::new("customer_data")
        .with_default_permission(Permission::None)
        .grant_permission("admin", Permission::Admin)
        .grant_permission("analyst", Permission::ReadRows)
        .grant_permission("tenant_user", Permission::ReadRows) // Tenant also needs permission
        .grant_permission("pii_analyst", Permission::ReadRows)
        .grant_group_permission("sales", Permission::ReadAggregate)
        .add_row_filter("tenant_user", RowFilter::equals("tenant_id", "tenant_123"))
        .restrict_column("ssn")
        .restrict_column("credit_card")
        .grant_column_access("pii_analyst", vec!["ssn".to_string()]);

    acl.register_policy(policy);

    // Test admin access
    let admin = Principal::user("admin");
    assert!(acl.can_access(&admin, "customer_data"));
    assert_eq!(acl.get_permission(&admin, "customer_data"), Permission::Admin);
    assert!(acl.get_row_filters(&admin, "customer_data").is_empty());

    // Test analyst access with column restrictions
    let analyst = Principal::user("analyst");
    assert!(acl.can_access(&analyst, "customer_data"));
    assert_eq!(
        acl.get_permission(&analyst, "customer_data"),
        Permission::ReadRows
    );

    // Analyst should have restricted columns
    let restricted = acl.get_restricted_columns(&analyst, "customer_data");
    assert!(restricted.contains("ssn"));
    assert!(restricted.contains("credit_card"));

    // Test tenant user with RLS
    let tenant = Principal::user("tenant_user");
    assert!(acl.can_access(&tenant, "customer_data"));
    let filters = acl.get_row_filters(&tenant, "customer_data");
    assert_eq!(filters.len(), 1);
    assert_eq!(filters[0].column, "tenant_id");

    // Test group member
    let sales_rep = Principal::user("rep1").with_groups(vec!["sales".to_string()]);
    assert!(acl.can_access(&sales_rep, "customer_data"));
    assert_eq!(
        acl.get_permission(&sales_rep, "customer_data"),
        Permission::ReadAggregate
    );

    // PII analyst can see SSN
    let pii_analyst = Principal::user("pii_analyst");
    let pii_restricted = acl.get_restricted_columns(&pii_analyst, "customer_data");
    assert!(!pii_restricted.contains("ssn"));
    assert!(pii_restricted.contains("credit_card"));

    // Unknown user denied
    let unknown = Principal::user("hacker");
    assert!(!acl.can_access(&unknown, "customer_data"));
}

#[test]
fn test_access_denied_error() {
    let acl = AccessControlManager::new(true);

    let policy = TableAccessPolicy::new("secret_data").with_default_permission(Permission::None);
    acl.register_policy(policy);

    let user = Principal::user("unauthorized");
    let mut request = BrokerRequest {
        table_name: "secret_data".to_string(),
        sql: "SELECT * FROM secret_data".to_string(),
        ..Default::default()
    };

    let result = acl.check_and_modify_request(&user, &mut request);
    assert!(result.is_err());

    match result {
        Err(BrokerError::AccessDenied { table, .. }) => {
            assert_eq!(table, "secret_data");
        }
        _ => panic!("Expected AccessDenied error"),
    }
}

#[test]
fn test_access_control_rls_modification() {
    let acl = AccessControlManager::new(true);

    let policy = TableAccessPolicy::new("orders")
        .with_default_permission(Permission::ReadRows)
        .add_row_filter("regional_manager", RowFilter::equals("region", "west"));

    acl.register_policy(policy);

    let regional = Principal::user("regional_manager");
    let mut request = BrokerRequest {
        table_name: "orders".to_string(),
        sql: "SELECT * FROM orders WHERE status = 'pending'".to_string(),
        ..Default::default()
    };

    let result = acl.check_and_modify_request(&regional, &mut request);
    assert!(result.is_ok());
    // The SQL should now include the RLS filter
    assert!(request.sql.contains("region = 'west'"));
}

// ============== Quota Integration Tests ==============

#[test]
fn test_quota_management_full_flow() {
    let config = QuotaConfig {
        enabled: true,
        global_qps_limit: 1000,
        default_user_qps_limit: 10,
        default_user_concurrent_limit: 5,
        ..Default::default()
    };

    let manager = QuotaManager::new(config);

    // Set custom limits for VIP user
    manager.set_user_qps_limit("vip_user", 100);
    manager.set_user_concurrent_limit("vip_user", 20);

    // Regular user queries should be allowed initially
    for i in 0..5 {
        let result = manager.check_quota("regular_user", "table1");
        assert!(
            result.is_ok(),
            "Should allow query {} for regular user",
            i + 1
        );
    }

    // VIP user has higher limit
    for _ in 0..20 {
        let result = manager.check_quota("vip_user", "table1");
        assert!(result.is_ok(), "VIP should have higher limit");
    }

    // Test concurrent limits
    let mut guards = Vec::new();
    for i in 0..5 {
        let guard = manager.try_acquire_query_slot("test_user", "table1");
        assert!(
            guard.is_ok(),
            "Should allow first {} concurrent queries",
            i + 1
        );
        guards.push(guard.unwrap());
    }

    // 6th concurrent should fail
    let over_limit = manager.try_acquire_query_slot("test_user", "table1");
    assert!(over_limit.is_err(), "Should deny 6th concurrent query");

    // Drop guards to release slots
    drop(guards);

    // Should be able to acquire again
    let after_drop = manager.try_acquire_query_slot("test_user", "table1");
    assert!(after_drop.is_ok(), "Should allow after releasing");
}

#[test]
fn test_quota_disabled() {
    let config = QuotaConfig {
        enabled: false,
        ..Default::default()
    };

    let manager = QuotaManager::new(config);

    // All queries should be allowed when disabled
    for _ in 0..100 {
        let result = manager.check_quota("any_user", "any_table");
        assert!(result.is_ok());
    }
}

#[test]
fn test_query_complexity_limits() {
    let config = QuotaConfig {
        max_query_complexity: 100,
        max_result_rows: 10000,
        max_query_timeout_secs: 60,
        ..Default::default()
    };

    let manager = QuotaManager::new(config);

    // Simple query should pass
    assert!(manager.check_query_complexity(50).is_ok());

    // Complex query should fail
    assert!(manager.check_query_complexity(150).is_err());

    // Result limit should be capped
    assert_eq!(manager.check_result_limit(5000).unwrap(), 5000);
    assert_eq!(manager.check_result_limit(50000).unwrap(), 10000);

    // Timeout should be capped
    assert_eq!(
        manager.check_timeout(30).unwrap(),
        Duration::from_secs(30)
    );
    assert_eq!(
        manager.check_timeout(300).unwrap(),
        Duration::from_secs(60)
    );
}

// ============== Query Cancellation Integration Tests ==============

#[test]
fn test_query_lifecycle_management() {
    let manager = QueryManager::new(Duration::from_secs(60), 1000);

    // Register queries from different users
    let q1 = manager.register_query("table1", "user1", None);
    let q2 = manager.register_query("table2", "user1", None);
    let _q3 = manager.register_query("table1", "user2", None);

    assert_eq!(manager.active_count(), 3);

    // Update states
    manager.update_state(q1.query_id, QueryState::Executing);
    manager.update_state(q2.query_id, QueryState::Reducing);

    assert_eq!(q1.state(), QueryState::Executing);
    assert_eq!(q2.state(), QueryState::Reducing);

    // Cancel a query
    assert!(manager.cancel_query(q1.query_id).is_ok());
    assert_eq!(q1.state(), QueryState::Cancelled);
    assert!(!q1.is_active());

    // Complete a query
    manager.complete_query(q2.query_id, true);
    assert_eq!(q2.state(), QueryState::Completed);

    // Query user's queries
    let user1_queries = manager.user_queries("user1");
    assert_eq!(user1_queries.len(), 2);

    // Query table's queries
    let table1_queries = manager.table_queries("table1");
    assert_eq!(table1_queries.len(), 2);

    // Cleanup completed
    manager.cleanup_completed_queries();
    assert_eq!(manager.total_count(), 1); // Only q3 remains active
}

#[tokio::test]
async fn test_cancellation_token_propagation() {
    let token = CancellationToken::new();
    let token_clone = token.clone();

    let handle = tokio::spawn(async move {
        tokio::select! {
            _ = token_clone.cancelled() => {
                "cancelled"
            }
            _ = tokio::time::sleep(Duration::from_secs(60)) => {
                "timeout"
            }
        }
    });

    // Give task time to start
    tokio::time::sleep(Duration::from_millis(10)).await;

    // Cancel the token
    token.cancel();

    let result = handle.await.unwrap();
    assert_eq!(result, "cancelled");
}

#[test]
fn test_query_timeout() {
    let manager = QueryManager::new(Duration::from_millis(10), 1000);
    let query = manager.register_query("table1", "user1", Some(Duration::from_millis(10)));

    // Wait for timeout
    std::thread::sleep(Duration::from_millis(50));

    // The query should be timed out
    assert!(query.is_timed_out());
}

// ============== Combined Integration Tests ==============

#[test]
fn test_broker_components_together() {
    // Initialize all components
    let routing_manager = RoutingManagerBuilder::new().build();
    let reduce_service = BrokerReduceService::default();
    let access_control = AccessControlManager::new(true);
    let quota_manager = QuotaManager::new(QuotaConfig::default());
    let query_manager = QueryManager::default();

    // Setup table with access control
    let policy =
        TableAccessPolicy::new("sales_data").with_default_permission(Permission::ReadRows);
    access_control.register_policy(policy);

    // Setup routing
    routing_manager.register_table("sales_data".to_string());
    let servers: Vec<ServerInstance> = (0..3)
        .map(|i| create_server(&format!("server{}", i), 8099 + i as u16))
        .collect();

    for i in 0..10 {
        routing_manager.update_segment_mapping(
            "sales_data",
            format!("segment_{}", i),
            servers.clone(),
        );
    }

    // Simulate query flow
    let user = Principal::user("analyst");
    let request = BrokerRequest::new(
        1,
        "SELECT * FROM sales_data LIMIT 100".to_string(),
        "sales_data".to_string(),
    );

    // 1. Check access control
    assert!(access_control.can_access(&user, "sales_data"));

    // 2. Check quota
    assert!(quota_manager.check_quota("analyst", "sales_data").is_ok());

    // 3. Register query
    let query =
        query_manager.register_query("sales_data", "analyst", Some(Duration::from_secs(30)));
    assert!(query.is_active());

    // 4. Get routing
    let routing = routing_manager.get_routing_table(&request, 1).unwrap();
    assert!(!routing.is_empty());

    // 5. Mark query state
    query_manager.update_state(query.query_id, QueryState::Executing);

    // 6. Simulate responses and reduce
    let responses: Vec<(ServerInstance, ServerResponse)> = (0..3)
        .map(|i| {
            let rows = vec![vec![DataValue::Long(100), DataValue::Double(100.0)]];
            create_server_response(servers[i].clone(), rows)
        })
        .collect();

    query_manager.update_state(query.query_id, QueryState::Reducing);
    let result = reduce_service.reduce(responses, 3);
    assert!(result.is_ok());

    // 7. Complete query
    query_manager.complete_query(query.query_id, true);
    assert_eq!(query.state(), QueryState::Completed);
}

// ============== Error Handling Integration Tests ==============

#[test]
fn test_error_propagation() {
    let routing_manager = RoutingManagerBuilder::new().build();

    // Query non-existent table
    let request = BrokerRequest::new(
        1,
        "SELECT * FROM missing".to_string(),
        "missing".to_string(),
    );
    let result = routing_manager.get_routing_table(&request, 1);
    assert!(result.is_err());

    match result {
        Err(BrokerError::TableNotFound(table)) => {
            assert_eq!(table, "missing");
        }
        _ => panic!("Expected TableNotFound error"),
    }
}

// ============== Stress Tests ==============

#[test]
fn test_high_volume_routing() {
    let manager = RoutingManagerBuilder::new().build();

    // Create large table with many segments
    manager.register_table("large_table".to_string());

    let servers: Vec<ServerInstance> = (0..20)
        .map(|i| create_server(&format!("server{}", i), 8099 + i as u16))
        .collect();

    for i in 0..1000 {
        let idx = i % servers.len();
        manager.update_segment_mapping(
            "large_table",
            format!("segment_{}", i),
            vec![
                servers[idx].clone(),
                servers[(idx + 1) % servers.len()].clone(),
            ],
        );
    }

    // Run many routing requests
    for i in 0..100 {
        let request = BrokerRequest::new(
            i as u64,
            format!("SELECT * FROM large_table WHERE id = {}", i),
            "large_table".to_string(),
        );
        let result = manager.get_routing_table(&request, i as u64);
        assert!(result.is_ok());
    }
}

#[test]
fn test_concurrent_quota_checking() {
    use std::thread;

    let manager = Arc::new(QuotaManager::new(QuotaConfig {
        enabled: true,
        global_qps_limit: 10000,
        default_user_qps_limit: 1000,
        default_table_qps_limit: 1000,
        default_user_concurrent_limit: 100,
        ..Default::default()
    }));

    let handles: Vec<_> = (0..10)
        .map(|i| {
            let mgr = manager.clone();
            thread::spawn(move || {
                for j in 0..100 {
                    let _ = mgr.check_quota(&format!("user{}", i), &format!("table{}", j % 5));
                }
            })
        })
        .collect();

    for h in handles {
        h.join().unwrap();
    }
}

#[test]
fn test_concurrent_query_registration() {
    use std::thread;

    let manager = Arc::new(QueryManager::new(Duration::from_secs(60), 10000));

    let handles: Vec<_> = (0..10)
        .map(|i| {
            let mgr = manager.clone();
            thread::spawn(move || {
                for j in 0..100 {
                    let query = mgr.register_query(
                        &format!("table{}", j % 5),
                        &format!("user{}", i),
                        None,
                    );
                    mgr.update_state(query.query_id, QueryState::Executing);
                    mgr.complete_query(query.query_id, true);
                }
            })
        })
        .collect();

    for h in handles {
        h.join().unwrap();
    }

    // Cleanup
    manager.cleanup_completed_queries();
    assert_eq!(manager.active_count(), 0);
}
