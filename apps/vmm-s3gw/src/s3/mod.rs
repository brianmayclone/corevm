pub mod error;
pub mod xml;
pub mod bucket;
pub mod object;
pub mod multipart;

use axum::Router;
use std::sync::Arc;
use crate::AppState;

pub fn router(state: Arc<AppState>) -> Router {
    Router::new()
        .merge(bucket::routes())
        .merge(object::routes())
        .merge(multipart::routes())
        .with_state(state)
}
