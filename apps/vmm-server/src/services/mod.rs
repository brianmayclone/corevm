//! Service layer — business logic separated from HTTP handlers.
//!
//! Each service encapsulates DB access + validation for a domain.
//! API handlers call services, services call the DB.

pub mod auth;
pub mod user;
pub mod vm;
pub mod storage;
