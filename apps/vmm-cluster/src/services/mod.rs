//! Service layer — business logic separated from HTTP handlers.
//!
//! The cluster is the central authority. Services own the data and
//! push changes to nodes via the node_client.

pub mod auth;
pub mod user;
pub mod audit;
pub mod event;
pub mod cluster;
pub mod host;
pub mod vm;
pub mod datastore;
pub mod task;
pub mod migration;
pub mod alarm;
