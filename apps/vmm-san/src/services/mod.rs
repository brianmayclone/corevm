//! Service layer — centralizes all database access.
//!
//! All SQL queries live here. API handlers, FUSE, and engines call these services
//! instead of writing raw SQL. This keeps business logic and data access separate.

pub mod volume;
pub mod chunk;
pub mod backend;
pub mod file;
pub mod peer;
pub mod disk;
pub mod benchmark;
