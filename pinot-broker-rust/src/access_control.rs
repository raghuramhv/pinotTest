//! Access Control Module for Pinot Broker
//!
//! Implements Row-Level Security (RLS) and Column-Level Security (CLS)
//! for fine-grained access control on queries.

use crate::error::{BrokerError, Result};
use crate::types::BrokerRequest;
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

// ============== Access Control Types ==============

/// Principal representing a user or service account
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Principal {
    /// Unique identifier for the principal
    pub id: String,
    /// Type of principal (user, service, group)
    pub principal_type: PrincipalType,
    /// Groups this principal belongs to
    pub groups: Vec<String>,
    /// Additional attributes for ABAC
    pub attributes: HashMap<String, String>,
}

#[derive(Debug, Clone, Hash, PartialEq, Eq, Serialize, Deserialize)]
pub enum PrincipalType {
    User,
    Service,
    Group,
}

impl Principal {
    pub fn user(id: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            principal_type: PrincipalType::User,
            groups: Vec::new(),
            attributes: HashMap::new(),
        }
    }

    pub fn service(id: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            principal_type: PrincipalType::Service,
            groups: Vec::new(),
            attributes: HashMap::new(),
        }
    }

    pub fn with_groups(mut self, groups: Vec<String>) -> Self {
        self.groups = groups;
        self
    }

    pub fn with_attribute(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.attributes.insert(key.into(), value.into());
        self
    }
}

/// Permission levels for access control
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum Permission {
    /// No access
    None,
    /// Can read aggregated data only
    ReadAggregate,
    /// Can read individual rows
    ReadRows,
    /// Can read all data including sensitive columns
    ReadAll,
    /// Full admin access
    Admin,
}

impl Default for Permission {
    fn default() -> Self {
        Permission::None
    }
}

/// Row-Level Security filter
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RowFilter {
    /// Column to filter on
    pub column: String,
    /// Filter operator
    pub operator: FilterOperator,
    /// Values to filter (supports multiple for IN operator)
    pub values: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum FilterOperator {
    Equals,
    NotEquals,
    In,
    NotIn,
    GreaterThan,
    LessThan,
    GreaterThanOrEqual,
    LessThanOrEqual,
    Contains,
    StartsWith,
}

impl RowFilter {
    pub fn equals(column: impl Into<String>, value: impl Into<String>) -> Self {
        Self {
            column: column.into(),
            operator: FilterOperator::Equals,
            values: vec![value.into()],
        }
    }

    pub fn in_list(column: impl Into<String>, values: Vec<String>) -> Self {
        Self {
            column: column.into(),
            operator: FilterOperator::In,
            values,
        }
    }

    /// Convert to SQL WHERE clause fragment
    pub fn to_sql(&self) -> String {
        let col = &self.column;
        match &self.operator {
            FilterOperator::Equals => format!("{} = '{}'", col, self.values[0]),
            FilterOperator::NotEquals => format!("{} != '{}'", col, self.values[0]),
            FilterOperator::In => {
                let vals: Vec<String> = self.values.iter().map(|v| format!("'{}'", v)).collect();
                format!("{} IN ({})", col, vals.join(", "))
            }
            FilterOperator::NotIn => {
                let vals: Vec<String> = self.values.iter().map(|v| format!("'{}'", v)).collect();
                format!("{} NOT IN ({})", col, vals.join(", "))
            }
            FilterOperator::GreaterThan => format!("{} > '{}'", col, self.values[0]),
            FilterOperator::LessThan => format!("{} < '{}'", col, self.values[0]),
            FilterOperator::GreaterThanOrEqual => format!("{} >= '{}'", col, self.values[0]),
            FilterOperator::LessThanOrEqual => format!("{} <= '{}'", col, self.values[0]),
            FilterOperator::Contains => format!("{} LIKE '%{}%'", col, self.values[0]),
            FilterOperator::StartsWith => format!("{} LIKE '{}%'", col, self.values[0]),
        }
    }
}

/// Access control policy for a table
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TableAccessPolicy {
    /// Table name
    pub table_name: String,
    /// Default permission for unauthenticated access
    pub default_permission: Permission,
    /// Principal-specific permissions
    pub principal_permissions: HashMap<String, Permission>,
    /// Group-specific permissions
    pub group_permissions: HashMap<String, Permission>,
    /// Row-level security filters by principal
    pub row_filters: HashMap<String, Vec<RowFilter>>,
    /// Row-level security filters by group
    pub group_row_filters: HashMap<String, Vec<RowFilter>>,
    /// Restricted columns (column-level security)
    pub restricted_columns: HashSet<String>,
    /// Column permissions by principal
    pub column_permissions: HashMap<String, HashSet<String>>,
    /// Columns visible to each group
    pub group_column_permissions: HashMap<String, HashSet<String>>,
}

impl TableAccessPolicy {
    pub fn new(table_name: impl Into<String>) -> Self {
        Self {
            table_name: table_name.into(),
            ..Default::default()
        }
    }

    pub fn with_default_permission(mut self, permission: Permission) -> Self {
        self.default_permission = permission;
        self
    }

    pub fn grant_permission(mut self, principal_id: impl Into<String>, permission: Permission) -> Self {
        self.principal_permissions.insert(principal_id.into(), permission);
        self
    }

    pub fn grant_group_permission(mut self, group: impl Into<String>, permission: Permission) -> Self {
        self.group_permissions.insert(group.into(), permission);
        self
    }

    pub fn add_row_filter(mut self, principal_id: impl Into<String>, filter: RowFilter) -> Self {
        self.row_filters
            .entry(principal_id.into())
            .or_insert_with(Vec::new)
            .push(filter);
        self
    }

    pub fn add_group_row_filter(mut self, group: impl Into<String>, filter: RowFilter) -> Self {
        self.group_row_filters
            .entry(group.into())
            .or_insert_with(Vec::new)
            .push(filter);
        self
    }

    pub fn restrict_column(mut self, column: impl Into<String>) -> Self {
        self.restricted_columns.insert(column.into());
        self
    }

    pub fn grant_column_access(
        mut self,
        principal_id: impl Into<String>,
        columns: Vec<String>,
    ) -> Self {
        let cols: HashSet<String> = columns.into_iter().collect();
        self.column_permissions.insert(principal_id.into(), cols);
        self
    }
}

// ============== Access Control Manager ==============

/// Manages access control policies across all tables
pub struct AccessControlManager {
    /// Table name -> Access policy
    policies: RwLock<HashMap<String, TableAccessPolicy>>,
    /// Whether access control is enabled
    enabled: bool,
    /// Audit logger callback
    audit_logger: Option<Arc<dyn Fn(&str, &Principal, &str) + Send + Sync>>,
}

impl AccessControlManager {
    pub fn new(enabled: bool) -> Self {
        Self {
            policies: RwLock::new(HashMap::new()),
            enabled,
            audit_logger: None,
        }
    }

    pub fn with_audit_logger<F>(mut self, logger: F) -> Self
    where
        F: Fn(&str, &Principal, &str) + Send + Sync + 'static,
    {
        self.audit_logger = Some(Arc::new(logger));
        self
    }

    /// Register an access policy for a table
    pub fn register_policy(&self, policy: TableAccessPolicy) {
        let mut policies = self.policies.write();
        policies.insert(policy.table_name.clone(), policy);
    }

    /// Remove access policy for a table
    pub fn remove_policy(&self, table_name: &str) {
        let mut policies = self.policies.write();
        policies.remove(table_name);
    }

    /// Get the effective permission for a principal on a table
    pub fn get_permission(&self, principal: &Principal, table_name: &str) -> Permission {
        if !self.enabled {
            return Permission::Admin;
        }

        let policies = self.policies.read();
        let policy = match policies.get(table_name) {
            Some(p) => p,
            None => return Permission::ReadAll, // No policy = open access
        };

        // Check principal-specific permission first
        if let Some(&perm) = policy.principal_permissions.get(&principal.id) {
            return perm;
        }

        // Check group permissions
        let mut best_perm = policy.default_permission;
        for group in &principal.groups {
            if let Some(&group_perm) = policy.group_permissions.get(group) {
                if group_perm > best_perm {
                    best_perm = group_perm;
                }
            }
        }

        best_perm
    }

    /// Check if a principal can access a table
    pub fn can_access(&self, principal: &Principal, table_name: &str) -> bool {
        self.get_permission(principal, table_name) > Permission::None
    }

    /// Get row filters to apply for a principal
    pub fn get_row_filters(&self, principal: &Principal, table_name: &str) -> Vec<RowFilter> {
        if !self.enabled {
            return Vec::new();
        }

        let policies = self.policies.read();
        let policy = match policies.get(table_name) {
            Some(p) => p,
            None => return Vec::new(),
        };

        let mut filters = Vec::new();

        // Add principal-specific filters
        if let Some(principal_filters) = policy.row_filters.get(&principal.id) {
            filters.extend(principal_filters.iter().cloned());
        }

        // Add group filters
        for group in &principal.groups {
            if let Some(group_filters) = policy.group_row_filters.get(group) {
                filters.extend(group_filters.iter().cloned());
            }
        }

        filters
    }

    /// Get restricted columns that should be hidden
    pub fn get_restricted_columns(
        &self,
        principal: &Principal,
        table_name: &str,
    ) -> HashSet<String> {
        if !self.enabled {
            return HashSet::new();
        }

        let policies = self.policies.read();
        let policy = match policies.get(table_name) {
            Some(p) => p,
            None => return HashSet::new(),
        };

        // Start with all restricted columns
        let mut restricted = policy.restricted_columns.clone();

        // Remove columns the principal has explicit access to
        if let Some(allowed) = policy.column_permissions.get(&principal.id) {
            for col in allowed {
                restricted.remove(col);
            }
        }

        // Remove columns their groups have access to
        for group in &principal.groups {
            if let Some(allowed) = policy.group_column_permissions.get(group) {
                for col in allowed {
                    restricted.remove(col);
                }
            }
        }

        restricted
    }

    /// Validate and potentially modify a query request based on access control
    pub fn check_and_modify_request(
        &self,
        principal: &Principal,
        request: &mut BrokerRequest,
    ) -> Result<()> {
        let table_name = &request.table_name;

        // Check basic access
        if !self.can_access(principal, table_name) {
            self.log_audit("ACCESS_DENIED", principal, table_name);
            return Err(BrokerError::AccessDenied {
                table: table_name.clone(),
                reason: "No access permission".to_string(),
            });
        }

        // Apply row-level security filters
        let row_filters = self.get_row_filters(principal, table_name);
        if !row_filters.is_empty() {
            // Append RLS filters to the query
            let rls_clause: Vec<String> = row_filters.iter().map(|f| f.to_sql()).collect();
            let combined = rls_clause.join(" AND ");

            // Modify the SQL query to include RLS filters
            if request.sql.to_uppercase().contains("WHERE") {
                request.sql = request.sql.replace(
                    "WHERE",
                    &format!("WHERE ({}) AND ", combined),
                );
            } else if request.sql.to_uppercase().contains("FROM") {
                // Add WHERE clause after FROM
                let from_pos = request.sql.to_uppercase().find("FROM").unwrap();
                let table_end = request.sql[from_pos..].find(|c: char| c.is_whitespace() && c != ' ')
                    .map(|p| from_pos + p)
                    .unwrap_or(request.sql.len());

                // Find a safe insertion point
                let insert_pos = if let Some(pos) = request.sql[table_end..].to_uppercase().find("GROUP BY") {
                    table_end + pos
                } else if let Some(pos) = request.sql[table_end..].to_uppercase().find("ORDER BY") {
                    table_end + pos
                } else if let Some(pos) = request.sql[table_end..].to_uppercase().find("LIMIT") {
                    table_end + pos
                } else {
                    request.sql.len()
                };

                let (before, after) = request.sql.split_at(insert_pos);
                request.sql = format!("{} WHERE {} {}", before.trim(), combined, after.trim());
            }
        }

        // Get columns to mask/hide
        let restricted = self.get_restricted_columns(principal, table_name);
        if !restricted.is_empty() {
            // Check if query references any restricted columns
            for col in &restricted {
                if request.sql.to_lowercase().contains(&col.to_lowercase()) {
                    self.log_audit("COLUMN_ACCESS_DENIED", principal, table_name);
                    return Err(BrokerError::AccessDenied {
                        table: table_name.clone(),
                        reason: format!("Access to column '{}' is restricted", col),
                    });
                }
            }
        }

        self.log_audit("ACCESS_GRANTED", principal, table_name);
        Ok(())
    }

    fn log_audit(&self, action: &str, principal: &Principal, table: &str) {
        if let Some(ref logger) = self.audit_logger {
            logger(action, principal, table);
        }
    }
}

impl Default for AccessControlManager {
    fn default() -> Self {
        Self::new(false)
    }
}

// ============== Tests ==============

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_policy() -> TableAccessPolicy {
        TableAccessPolicy::new("test_table")
            .with_default_permission(Permission::None)
            .grant_permission("admin_user", Permission::Admin)
            .grant_permission("read_user", Permission::ReadRows)
            .grant_group_permission("analysts", Permission::ReadAggregate)
            .add_row_filter("limited_user", RowFilter::equals("tenant_id", "tenant1"))
            .restrict_column("ssn")
            .restrict_column("credit_card")
            .grant_column_access("pii_viewer", vec!["ssn".to_string()])
    }

    #[test]
    fn test_principal_creation() {
        let user = Principal::user("john")
            .with_groups(vec!["analysts".to_string(), "engineers".to_string()])
            .with_attribute("department", "engineering");

        assert_eq!(user.id, "john");
        assert_eq!(user.principal_type, PrincipalType::User);
        assert_eq!(user.groups.len(), 2);
        assert_eq!(user.attributes.get("department"), Some(&"engineering".to_string()));
    }

    #[test]
    fn test_row_filter_to_sql() {
        let filter = RowFilter::equals("tenant_id", "acme");
        assert_eq!(filter.to_sql(), "tenant_id = 'acme'");

        let in_filter = RowFilter::in_list("status", vec!["active".to_string(), "pending".to_string()]);
        assert_eq!(in_filter.to_sql(), "status IN ('active', 'pending')");
    }

    #[test]
    fn test_access_control_disabled() {
        let manager = AccessControlManager::new(false);
        manager.register_policy(create_test_policy());

        let user = Principal::user("anyone");
        assert_eq!(manager.get_permission(&user, "test_table"), Permission::Admin);
    }

    #[test]
    fn test_principal_permission() {
        let manager = AccessControlManager::new(true);
        manager.register_policy(create_test_policy());

        let admin = Principal::user("admin_user");
        assert_eq!(manager.get_permission(&admin, "test_table"), Permission::Admin);

        let reader = Principal::user("read_user");
        assert_eq!(manager.get_permission(&reader, "test_table"), Permission::ReadRows);

        let unknown = Principal::user("unknown");
        assert_eq!(manager.get_permission(&unknown, "test_table"), Permission::None);
    }

    #[test]
    fn test_group_permission() {
        let manager = AccessControlManager::new(true);
        manager.register_policy(create_test_policy());

        let analyst = Principal::user("analyst1").with_groups(vec!["analysts".to_string()]);
        assert_eq!(manager.get_permission(&analyst, "test_table"), Permission::ReadAggregate);
    }

    #[test]
    fn test_row_filters() {
        let manager = AccessControlManager::new(true);
        manager.register_policy(create_test_policy());

        let limited = Principal::user("limited_user");
        let filters = manager.get_row_filters(&limited, "test_table");

        assert_eq!(filters.len(), 1);
        assert_eq!(filters[0].column, "tenant_id");
    }

    #[test]
    fn test_column_restrictions() {
        let manager = AccessControlManager::new(true);
        manager.register_policy(create_test_policy());

        // Regular user should have restricted columns
        let regular = Principal::user("regular_user");
        let restricted = manager.get_restricted_columns(&regular, "test_table");
        assert!(restricted.contains("ssn"));
        assert!(restricted.contains("credit_card"));

        // PII viewer should have access to SSN
        let pii_viewer = Principal::user("pii_viewer");
        let pii_restricted = manager.get_restricted_columns(&pii_viewer, "test_table");
        assert!(!pii_restricted.contains("ssn"));
        assert!(pii_restricted.contains("credit_card"));
    }

    #[test]
    fn test_can_access() {
        let manager = AccessControlManager::new(true);
        manager.register_policy(create_test_policy());

        let admin = Principal::user("admin_user");
        assert!(manager.can_access(&admin, "test_table"));

        let unknown = Principal::user("unknown");
        assert!(!manager.can_access(&unknown, "test_table"));
    }

    #[test]
    fn test_no_policy_means_open_access() {
        let manager = AccessControlManager::new(true);

        let anyone = Principal::user("anyone");
        assert_eq!(manager.get_permission(&anyone, "unprotected_table"), Permission::ReadAll);
    }

    #[test]
    fn test_check_and_modify_request_denied() {
        let manager = AccessControlManager::new(true);
        manager.register_policy(create_test_policy());

        let unknown = Principal::user("unknown");
        let mut request = BrokerRequest {
            table_name: "test_table".to_string(),
            sql: "SELECT * FROM test_table".to_string(),
            ..Default::default()
        };

        let result = manager.check_and_modify_request(&unknown, &mut request);
        assert!(result.is_err());
    }

    #[test]
    fn test_check_and_modify_request_with_rls() {
        let manager = AccessControlManager::new(true);
        manager.register_policy(
            TableAccessPolicy::new("test_table")
                .with_default_permission(Permission::ReadRows)
                .add_row_filter("tenant_user", RowFilter::equals("tenant_id", "t1"))
        );

        let tenant_user = Principal::user("tenant_user");
        let mut request = BrokerRequest {
            table_name: "test_table".to_string(),
            sql: "SELECT * FROM test_table WHERE status = 'active'".to_string(),
            ..Default::default()
        };

        let result = manager.check_and_modify_request(&tenant_user, &mut request);
        assert!(result.is_ok());
        assert!(request.sql.contains("tenant_id = 't1'"));
    }

    #[test]
    fn test_restricted_column_access_denied() {
        let manager = AccessControlManager::new(true);
        manager.register_policy(create_test_policy());

        let reader = Principal::user("read_user");
        let mut request = BrokerRequest {
            table_name: "test_table".to_string(),
            sql: "SELECT name, ssn FROM test_table".to_string(),
            ..Default::default()
        };

        let result = manager.check_and_modify_request(&reader, &mut request);
        assert!(result.is_err());
        if let Err(BrokerError::AccessDenied { reason, .. }) = result {
            assert!(reason.contains("ssn"));
        }
    }

    #[test]
    fn test_permission_ordering() {
        assert!(Permission::Admin > Permission::ReadAll);
        assert!(Permission::ReadAll > Permission::ReadRows);
        assert!(Permission::ReadRows > Permission::ReadAggregate);
        assert!(Permission::ReadAggregate > Permission::None);
    }
}
