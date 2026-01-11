//! Transport module for server communication.
//!
//! Handles:
//! - Connection pooling to Pinot servers
//! - Async query execution
//! - Request serialization/deserialization
//! - Scatter-gather operations

pub mod server_channel;
pub mod query_router;
pub mod async_response;

pub use server_channel::{ServerChannel, ServerChannels};
pub use query_router::QueryRouter;
pub use async_response::AsyncQueryResponse;
