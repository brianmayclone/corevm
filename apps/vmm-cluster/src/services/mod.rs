//! Service layer — business logic separated from HTTP handlers.
//!
//! The cluster is the central authority. Services own the data and
//! push changes to nodes via the node_client.
//!
//! Architecture:
//! - BaseService: common DB helpers (count, sum, exists)
//! - ServiceRegistry: IoC container for discovering services
//! - Individual services: domain-specific logic

pub mod base;
pub mod registry;
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
pub mod stats;
pub mod drs_service;
pub mod storage_compat;
pub mod resource_group;
