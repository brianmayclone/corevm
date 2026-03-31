use axum::body::Bytes;
use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{delete, get, head, put};
use axum::Router;
use std::collections::HashMap;
use std::sync::Arc;
use vmm_core::san_mgmt::MgmtCommand;

use crate::auth;
use crate::s3::error::S3Error;
use crate::s3::xml;
use crate::AppState;

pub fn routes() -> Router<Arc<AppState>> {
    Router::new()
        .route("/", get(list_buckets))
        .route("/{bucket}", get(list_objects_v2_bucket))
        .route("/{bucket}", put(create_bucket))
        .route("/{bucket}", delete(delete_bucket))
        .route("/{bucket}", head(head_bucket))
}

async fn list_buckets(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Response, S3Error> {
    let auth_result = auth::validate_request(
        &state,
        &headers,
        "GET",
        "/",
        headers
            .get("x-amz-content-sha256")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("UNSIGNED-PAYLOAD"),
    )
    .await?;

    let resp = state
        .socket
        .mgmt_request(MgmtCommand::ListVolumes, &[], &[])
        .await
        .map_err(|e| S3Error::internal_error(format!("list volumes: {}", e)))?;

    if !resp.is_ok() {
        return Err(S3Error::internal_error("failed to list volumes"));
    }

    let volumes = resp.body_json();
    let volumes_arr = volumes.as_array().unwrap_or(&Vec::new()).clone();

    let buckets: Vec<xml::BucketInfo> = volumes_arr
        .iter()
        .map(|v| xml::BucketInfo {
            name: v["name"].as_str().unwrap_or("").to_string(),
            creation_date: v["created_at"].as_str().unwrap_or("").to_string(),
        })
        .collect();

    let body = xml::list_buckets_xml(&buckets, &auth_result.user_id);

    Ok((
        StatusCode::OK,
        [("content-type", "application/xml")],
        body,
    )
        .into_response())
}

async fn list_objects_v2_bucket(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(bucket): Path<String>,
    Query(params): Query<HashMap<String, String>>,
) -> Result<Response, S3Error> {
    crate::s3::object::handle_list_objects_v2_public(&state, &headers, &bucket, &params).await
}

async fn create_bucket(
    State(_state): State<Arc<AppState>>,
    Path(_bucket): Path<String>,
) -> Result<Response, S3Error> {
    Err(S3Error::internal_error(
        "Use CoreSAN API to create volumes",
    ))
}

async fn delete_bucket(
    State(_state): State<Arc<AppState>>,
    Path(_bucket): Path<String>,
) -> Result<Response, S3Error> {
    Err(S3Error::internal_error(
        "Use CoreSAN API to delete volumes",
    ))
}

async fn head_bucket(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(bucket): Path<String>,
) -> Result<Response, S3Error> {
    let _auth = auth::validate_request(
        &state,
        &headers,
        "HEAD",
        &format!("/{}", bucket),
        headers
            .get("x-amz-content-sha256")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("UNSIGNED-PAYLOAD"),
    )
    .await?;

    let resp = state
        .socket
        .mgmt_request(MgmtCommand::ResolveVolume, bucket.as_bytes(), &[])
        .await
        .map_err(|e| S3Error::internal_error(format!("resolve volume: {}", e)))?;

    if resp.is_ok() {
        Ok(StatusCode::OK.into_response())
    } else {
        Err(S3Error::no_such_bucket(bucket))
    }
}
