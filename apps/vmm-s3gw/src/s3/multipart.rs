use axum::body::Bytes;
use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::post;
use axum::Router;
use std::collections::HashMap;
use std::sync::Arc;
use vmm_core::san_object::ObjectCommand;

use crate::auth;
use crate::s3::error::S3Error;
use crate::s3::object::{resolve_bucket, status_to_s3_error};
use crate::s3::xml;
use crate::AppState;

pub fn routes() -> Router<Arc<AppState>> {
    Router::new().route("/{bucket}/{*key}", post(post_object))
}

async fn post_object(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path((bucket, key)): Path<(String, String)>,
    Query(params): Query<HashMap<String, String>>,
    body: Bytes,
) -> Result<Response, S3Error> {
    let _auth = auth::validate_request(
        &state,
        &headers,
        "POST",
        &format!("/{}/{}", bucket, key),
        headers
            .get("x-amz-content-sha256")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("UNSIGNED-PAYLOAD"),
    )
    .await?;

    let volume_id = resolve_bucket(&state, &bucket).await?;

    if params.contains_key("uploads") {
        // Initiate multipart upload
        let resp = state
            .socket
            .object_request(&volume_id, ObjectCommand::InitMultipart, key.as_bytes(), &[])
            .await
            .map_err(|e| S3Error::internal_error(format!("initiate multipart: {}", e)))?;

        if !resp.is_ok() {
            return Err(status_to_s3_error(resp.status, &bucket, &key));
        }

        let meta = resp.metadata_json();
        let upload_id = meta["upload_id"].as_str().unwrap_or("").to_string();

        let xml_body = xml::initiate_multipart_xml(&bucket, &key, &upload_id);

        return Ok((
            StatusCode::OK,
            [("content-type", "application/xml")],
            xml_body,
        )
            .into_response());
    }

    if let Some(upload_id) = params.get("uploadId") {
        // Complete multipart upload
        let parts = parse_complete_multipart_xml(&body);

        let complete_key = serde_json::json!({
            "upload_id": upload_id,
            "parts": parts,
        });

        let resp = state
            .socket
            .object_request(
                &volume_id,
                ObjectCommand::CompleteMultipart,
                complete_key.to_string().as_bytes(),
                &[],
            )
            .await
            .map_err(|e| S3Error::internal_error(format!("complete multipart: {}", e)))?;

        if !resp.is_ok() {
            return Err(status_to_s3_error(resp.status, &bucket, &key));
        }

        let meta = resp.metadata_json();
        let etag = meta["etag"].as_str().unwrap_or("\"\"").to_string();

        let xml_body = xml::complete_multipart_xml(&bucket, &key, &etag);

        return Ok((
            StatusCode::OK,
            [("content-type", "application/xml")],
            xml_body,
        )
            .into_response());
    }

    Err(S3Error::invalid_argument(
        "POST requires ?uploads or ?uploadId parameter",
    ))
}

/// Parse the CompleteMultipartUpload XML body to extract Part elements.
///
/// Expected format:
/// ```xml
/// <CompleteMultipartUpload>
///   <Part><PartNumber>1</PartNumber><ETag>"..."</ETag></Part>
///   ...
/// </CompleteMultipartUpload>
/// ```
fn parse_complete_multipart_xml(body: &[u8]) -> Vec<serde_json::Value> {
    let text = String::from_utf8_lossy(body);
    let mut parts = Vec::new();

    // Simple XML parsing without a full XML parser dependency
    let mut remaining = text.as_ref();
    while let Some(start) = remaining.find("<Part>") {
        let after_start = &remaining[start + 6..];
        if let Some(end) = after_start.find("</Part>") {
            let part_content = &after_start[..end];

            let part_number = extract_xml_value(part_content, "PartNumber")
                .and_then(|s| s.parse::<u64>().ok())
                .unwrap_or(0);
            let etag = extract_xml_value(part_content, "ETag").unwrap_or_default();

            parts.push(serde_json::json!({
                "part_number": part_number,
                "etag": etag,
            }));

            remaining = &after_start[end + 7..];
        } else {
            break;
        }
    }

    parts
}

fn extract_xml_value<'a>(content: &'a str, tag: &str) -> Option<String> {
    let open = format!("<{}>", tag);
    let close = format!("</{}>", tag);
    if let Some(start) = content.find(&open) {
        let after = &content[start + open.len()..];
        if let Some(end) = after.find(&close) {
            return Some(after[..end].to_string());
        }
    }
    None
}
