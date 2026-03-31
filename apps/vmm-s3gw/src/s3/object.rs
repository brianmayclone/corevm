use axum::body::Bytes;
use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{delete, get, head, put};
use axum::Router;
use std::collections::HashMap;
use std::sync::Arc;
use vmm_core::san_mgmt::MgmtCommand;
use vmm_core::san_object::ObjectCommand;

use crate::auth;
use crate::s3::error::S3Error;
use crate::s3::xml;
use crate::AppState;

pub fn routes() -> Router<Arc<AppState>> {
    Router::new()
        .route("/{bucket}/{*key}", put(put_object))
        .route("/{bucket}/{*key}", get(get_object))
        .route("/{bucket}/{*key}", delete(delete_object))
        .route("/{bucket}/{*key}", head(head_object))
}

/// Resolve a bucket name to a volume ID via the management socket.
pub async fn resolve_bucket(state: &AppState, bucket: &str) -> Result<String, S3Error> {
    let resp = state
        .socket
        .mgmt_request(MgmtCommand::ResolveVolume, bucket.as_bytes(), &[])
        .await
        .map_err(|e| S3Error::internal_error(format!("resolve volume: {}", e)))?;

    if !resp.is_ok() {
        return Err(S3Error::no_such_bucket(bucket));
    }

    let meta = resp.metadata_json();
    let volume_id = meta["id"]
        .as_str()
        .ok_or_else(|| S3Error::internal_error("missing volume id in response"))?
        .to_string();

    Ok(volume_id)
}

/// Handle ListObjectsV2 — called from bucket routes and get_object.
pub async fn handle_list_objects_v2_public(
    state: &AppState,
    headers: &HeaderMap,
    bucket: &str,
    params: &HashMap<String, String>,
) -> Result<Response, S3Error> {
    let _auth = auth::validate_request(
        state,
        headers,
        "GET",
        &format!("/{}", bucket),
        headers
            .get("x-amz-content-sha256")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("UNSIGNED-PAYLOAD"),
    )
    .await?;

    let volume_id = resolve_bucket(state, bucket).await?;

    let prefix = params.get("prefix").cloned().unwrap_or_default();
    let max_keys: usize = params
        .get("max-keys")
        .and_then(|v| v.parse().ok())
        .unwrap_or(1000);
    let continuation_token = params.get("continuation-token").cloned().unwrap_or_default();

    let list_key = serde_json::json!({
        "prefix": prefix,
        "marker": continuation_token,
        "max_keys": max_keys,
    });

    let resp = state
        .socket
        .object_request(
            &volume_id,
            ObjectCommand::List,
            list_key.to_string().as_bytes(),
            &[],
        )
        .await
        .map_err(|e| S3Error::internal_error(format!("list objects: {}", e)))?;

    if !resp.is_ok() {
        return Err(status_to_s3_error(resp.status, bucket, &prefix));
    }

    let body_json = resp.body_json();
    let objects_arr = body_json.as_array().cloned().unwrap_or_default();

    let objects: Vec<xml::ObjectInfo> = objects_arr
        .iter()
        .map(|o| xml::ObjectInfo {
            key: o["key"].as_str().unwrap_or("").to_string(),
            last_modified: o["last_modified"].as_str().unwrap_or("").to_string(),
            etag: o["etag"].as_str().unwrap_or("").to_string(),
            size: o["size"].as_u64().unwrap_or(0),
            storage_class: "STANDARD".to_string(),
        })
        .collect();

    let is_truncated = objects.len() >= max_keys;
    let next_token = if is_truncated {
        objects.last().map(|o| o.key.clone()).unwrap_or_default()
    } else {
        String::new()
    };

    let body = xml::list_objects_v2_xml(
        bucket,
        &prefix,
        &objects,
        is_truncated,
        objects.len(),
        max_keys,
        &continuation_token,
        &next_token,
    );

    Ok((
        StatusCode::OK,
        [("content-type", "application/xml")],
        body,
    )
        .into_response())
}

async fn put_object(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path((bucket, key)): Path<(String, String)>,
    body: Bytes,
) -> Result<Response, S3Error> {
    let _auth = auth::validate_request(
        &state,
        &headers,
        "PUT",
        &format!("/{}/{}", bucket, key),
        headers
            .get("x-amz-content-sha256")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("UNSIGNED-PAYLOAD"),
    )
    .await?;

    let volume_id = resolve_bucket(&state, &bucket).await?;

    // Check for copy source
    if let Some(copy_source) = headers.get("x-amz-copy-source").and_then(|v| v.to_str().ok()) {
        // Copy object: x-amz-copy-source = /source-bucket/source-key
        let source = copy_source.trim_start_matches('/');
        let (src_bucket, src_key) = source
            .split_once('/')
            .ok_or_else(|| S3Error::invalid_argument("invalid x-amz-copy-source"))?;

        let src_volume_id = resolve_bucket(&state, src_bucket).await?;

        let copy_key = serde_json::json!({
            "src_key": src_key,
            "dst_key": key,
        });

        // If cross-bucket, we'd need more logic; for same-bucket:
        let resp = state
            .socket
            .object_request(
                &volume_id,
                ObjectCommand::Copy,
                copy_key.to_string().as_bytes(),
                &[],
            )
            .await
            .map_err(|e| S3Error::internal_error(format!("copy object: {}", e)))?;

        if !resp.is_ok() {
            return Err(status_to_s3_error(resp.status, &bucket, &key));
        }

        let meta = resp.metadata_json();
        let etag = meta["etag"].as_str().unwrap_or("\"\"").to_string();
        let last_modified = meta["last_modified"].as_str().unwrap_or("").to_string();

        let xml_body = xml::copy_object_result_xml(&etag, &last_modified);

        return Ok((
            StatusCode::OK,
            [("content-type", "application/xml")],
            xml_body,
        )
            .into_response());
    }

    // Regular PUT
    let resp = state
        .socket
        .object_request(&volume_id, ObjectCommand::Put, key.as_bytes(), &body)
        .await
        .map_err(|e| S3Error::internal_error(format!("put object: {}", e)))?;

    if !resp.is_ok() {
        return Err(status_to_s3_error(resp.status, &bucket, &key));
    }

    let meta = resp.metadata_json();
    let etag = meta["etag"].as_str().unwrap_or("\"\"").to_string();

    Ok((StatusCode::OK, [("etag", etag.as_str())], "").into_response())
}

async fn get_object(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path((bucket, key)): Path<(String, String)>,
    Query(params): Query<HashMap<String, String>>,
) -> Result<Response, S3Error> {
    // Check if this is actually a ListObjectsV2 request (list-type=2 on bucket)
    if params.contains_key("list-type") {
        return handle_list_objects_v2_public(&state, &headers, &bucket, &params).await;
    }

    let _auth = auth::validate_request(
        &state,
        &headers,
        "GET",
        &format!("/{}/{}", bucket, key),
        headers
            .get("x-amz-content-sha256")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("UNSIGNED-PAYLOAD"),
    )
    .await?;

    let volume_id = resolve_bucket(&state, &bucket).await?;

    let resp = state
        .socket
        .object_request(&volume_id, ObjectCommand::Get, key.as_bytes(), &[])
        .await
        .map_err(|e| S3Error::internal_error(format!("get object: {}", e)))?;

    if !resp.is_ok() {
        return Err(status_to_s3_error(resp.status, &bucket, &key));
    }

    let meta = resp.metadata_json();
    let content_type = meta["content_type"]
        .as_str()
        .unwrap_or("application/octet-stream")
        .to_string();
    let etag = meta["etag"].as_str().unwrap_or("").to_string();
    let content_length = resp.body.len().to_string();

    Ok((
        StatusCode::OK,
        [
            ("content-type", content_type.as_str()),
            ("etag", etag.as_str()),
            ("content-length", content_length.as_str()),
        ],
        resp.body,
    )
        .into_response())
}

async fn head_object(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path((bucket, key)): Path<(String, String)>,
) -> Result<Response, S3Error> {
    let _auth = auth::validate_request(
        &state,
        &headers,
        "HEAD",
        &format!("/{}/{}", bucket, key),
        headers
            .get("x-amz-content-sha256")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("UNSIGNED-PAYLOAD"),
    )
    .await?;

    let volume_id = resolve_bucket(&state, &bucket).await?;

    let resp = state
        .socket
        .object_request(&volume_id, ObjectCommand::Head, key.as_bytes(), &[])
        .await
        .map_err(|e| S3Error::internal_error(format!("head object: {}", e)))?;

    if !resp.is_ok() {
        return Err(status_to_s3_error(resp.status, &bucket, &key));
    }

    let meta = resp.metadata_json();
    let content_type = meta["content_type"]
        .as_str()
        .unwrap_or("application/octet-stream")
        .to_string();
    let etag = meta["etag"].as_str().unwrap_or("").to_string();
    let size = meta["size"].as_u64().unwrap_or(0).to_string();

    Ok((
        StatusCode::OK,
        [
            ("content-type", content_type.as_str()),
            ("etag", etag.as_str()),
            ("content-length", size.as_str()),
        ],
        "",
    )
        .into_response())
}

async fn delete_object(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path((bucket, key)): Path<(String, String)>,
) -> Result<Response, S3Error> {
    let _auth = auth::validate_request(
        &state,
        &headers,
        "DELETE",
        &format!("/{}/{}", bucket, key),
        headers
            .get("x-amz-content-sha256")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("UNSIGNED-PAYLOAD"),
    )
    .await?;

    let volume_id = resolve_bucket(&state, &bucket).await?;

    let resp = state
        .socket
        .object_request(&volume_id, ObjectCommand::Delete, key.as_bytes(), &[])
        .await
        .map_err(|e| S3Error::internal_error(format!("delete object: {}", e)))?;

    if !resp.is_ok() {
        return Err(status_to_s3_error(resp.status, &bucket, &key));
    }

    Ok(StatusCode::NO_CONTENT.into_response())
}

/// Convert a SAN object status code to an S3Error.
pub fn status_to_s3_error(status: u32, bucket: &str, key: &str) -> S3Error {
    use vmm_core::san_object::ObjectStatus;
    match ObjectStatus::from_u32(status) {
        Some(ObjectStatus::NotFound) => S3Error::no_such_key(format!("{}/{}", bucket, key)),
        Some(ObjectStatus::AccessDenied) => S3Error::access_denied("Access denied"),
        Some(ObjectStatus::NoSpace) => S3Error::insufficient_storage("No space left"),
        Some(ObjectStatus::InvalidKey) => S3Error::invalid_argument("Invalid key"),
        Some(ObjectStatus::LeaseDenied) => S3Error::slow_down("Resource busy, try again"),
        _ => S3Error::internal_error(format!("object backend error: status={}", status)),
    }
}
