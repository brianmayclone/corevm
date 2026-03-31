use axum::http::HeaderMap;
use sha2::{Digest, Sha256};
use vmm_core::san_mgmt::MgmtCommand;

use crate::s3::error::S3Error;
use crate::AppState;

pub struct S3Auth {
    pub access_key: String,
    pub user_id: String,
}

/// Parse the AWS Signature V4 Authorization header and validate credentials
/// via the management socket.
pub async fn validate_request(
    state: &AppState,
    headers: &HeaderMap,
    method: &str,
    uri: &str,
    payload_hash: &str,
) -> Result<S3Auth, S3Error> {
    let auth_header = headers
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .ok_or_else(|| S3Error::access_denied("Missing Authorization header"))?;

    // Parse: AWS4-HMAC-SHA256 Credential=AKID/date/region/s3/aws4_request, SignedHeaders=..., Signature=...
    if !auth_header.starts_with("AWS4-HMAC-SHA256 ") {
        return Err(S3Error::access_denied(
            "Unsupported auth scheme",
        ));
    }

    let parts_str = &auth_header["AWS4-HMAC-SHA256 ".len()..];

    let mut credential = "";
    let mut signed_headers = "";
    let mut signature = "";

    for part in parts_str.split(", ") {
        if let Some(val) = part.strip_prefix("Credential=") {
            credential = val;
        } else if let Some(val) = part.strip_prefix("SignedHeaders=") {
            signed_headers = val;
        } else if let Some(val) = part.strip_prefix("Signature=") {
            signature = val;
        }
    }

    if credential.is_empty() || signature.is_empty() {
        return Err(S3Error::access_denied(
            "Malformed Authorization header",
        ));
    }

    // credential = AKID/20260101/us-east-1/s3/aws4_request
    let cred_parts: Vec<&str> = credential.splitn(5, '/').collect();
    if cred_parts.len() < 5 {
        return Err(S3Error::access_denied(
            "Malformed Credential",
        ));
    }
    let access_key = cred_parts[0];
    let date = cred_parts[1];
    let region = cred_parts[2];

    // Build canonical request
    let canonical_headers = build_canonical_headers(headers, signed_headers);
    let canonical_request = format!(
        "{}\n{}\n{}\n{}\n{}\n{}",
        method,
        uri,
        "", // query string (simplified - not parsing query from uri)
        canonical_headers,
        signed_headers,
        payload_hash
    );

    let canonical_hash = hex_sha256(canonical_request.as_bytes());

    let string_to_sign = format!(
        "AWS4-HMAC-SHA256\n{}\n{}/{}/{}/aws4_request\n{}",
        headers
            .get("x-amz-date")
            .and_then(|v| v.to_str().ok())
            .unwrap_or(""),
        date,
        region,
        "s3",
        canonical_hash
    );

    // Validate via management socket
    let validate_body = serde_json::json!({
        "access_key": access_key,
        "string_to_sign": string_to_sign,
        "signature": signature,
        "region": region,
        "date": date,
    });

    let resp = state
        .socket
        .mgmt_request(
            MgmtCommand::ValidateCredential,
            &[],
            validate_body.to_string().as_bytes(),
        )
        .await
        .map_err(|e| S3Error::internal_error(format!("auth backend: {}", e)))?;

    if !resp.is_ok() {
        return Err(S3Error::access_denied("Invalid credentials"));
    }

    let resp_json = resp.body_json();
    let user_id = resp_json["user_id"]
        .as_str()
        .unwrap_or(access_key)
        .to_string();

    Ok(S3Auth {
        access_key: access_key.to_string(),
        user_id,
    })
}

fn build_canonical_headers(headers: &HeaderMap, signed_headers: &str) -> String {
    let mut result = String::new();
    for name in signed_headers.split(';') {
        let value = headers
            .get(name)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("");
        result.push_str(name);
        result.push(':');
        result.push_str(value.trim());
        result.push('\n');
    }
    result
}

fn hex_sha256(data: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(data);
    let result = hasher.finalize();
    hex_encode(&result)
}

fn hex_encode(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        s.push_str(&format!("{:02x}", b));
    }
    s
}
