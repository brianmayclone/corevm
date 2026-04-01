//! S3 credential management REST endpoints.
//! Used by vmm-ui and vmm-cluster to create/list/delete S3 access keys.

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::Json;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use crate::state::CoreSanState;

#[derive(Deserialize)]
pub struct CreateCredentialRequest {
    pub user_id: String,
    #[serde(default)]
    pub display_name: String,
}

#[derive(Serialize)]
pub struct CreateCredentialResponse {
    pub id: String,
    pub access_key: String,
    pub secret_key: String,
}

#[derive(Serialize)]
pub struct CredentialResponse {
    pub id: String,
    pub access_key: String,
    pub user_id: String,
    pub display_name: String,
    pub status: String,
    pub created_at: String,
    pub expires_at: Option<String>,
}

/// GET /api/s3/credentials
pub async fn list(
    State(state): State<Arc<CoreSanState>>,
) -> Result<Json<Vec<CredentialResponse>>, (StatusCode, String)> {
    let db = state.db.read();
    let mut stmt = db.prepare(
        "SELECT id, access_key, user_id, display_name, status, created_at, expires_at FROM s3_credentials"
    ).map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    let creds: Vec<CredentialResponse> = stmt.query_map([], |row| {
        Ok(CredentialResponse {
            id: row.get(0)?,
            access_key: row.get(1)?,
            user_id: row.get(2)?,
            display_name: row.get(3)?,
            status: row.get(4)?,
            created_at: row.get(5)?,
            expires_at: row.get(6)?,
        })
    }).unwrap().filter_map(|r| r.ok()).collect();

    Ok(Json(creds))
}

/// POST /api/s3/credentials
pub async fn create(
    State(state): State<Arc<CoreSanState>>,
    Json(body): Json<CreateCredentialRequest>,
) -> Result<(StatusCode, Json<CreateCredentialResponse>), (StatusCode, String)> {
    use rand::Rng;
    let mut rng = rand::thread_rng();

    let access_key: String = (0..20).map(|_| {
        let idx = rng.gen_range(0..36u8);
        if idx < 10 { (b'0' + idx) as char } else { (b'A' + idx - 10) as char }
    }).collect();

    let secret_key: String = (0..40).map(|_| {
        let idx = rng.gen_range(0..62u8);
        if idx < 10 { (b'0' + idx) as char }
        else if idx < 36 { (b'A' + idx - 10) as char }
        else { (b'a' + idx - 36) as char }
    }).collect();

    use sha2::{Sha256, Digest};
    let key_hash = Sha256::digest(state.node_id.as_bytes());
    let encrypted: Vec<u8> = secret_key.bytes()
        .zip(key_hash.iter().cycle())
        .map(|(b, k)| b ^ k)
        .collect();
    let secret_key_enc = crate::engine::mgmt_server::base64_encode(&encrypted);

    let id = uuid::Uuid::new_v4().to_string();
    let db = state.db.write();
    db.execute(
        "INSERT INTO s3_credentials (id, access_key, secret_key_enc, user_id, display_name)
         VALUES (?1, ?2, ?3, ?4, ?5)",
        rusqlite::params![&id, &access_key, &secret_key_enc, &body.user_id, &body.display_name],
    ).map_err(|e| (StatusCode::CONFLICT, format!("Failed to create credential: {}", e)))?;

    tracing::info!("S3 credential created: access_key={} user={}", access_key, body.user_id);

    Ok((StatusCode::CREATED, Json(CreateCredentialResponse { id, access_key, secret_key })))
}

/// DELETE /api/s3/credentials/{id}
pub async fn delete(
    State(state): State<Arc<CoreSanState>>,
    Path(id): Path<String>,
) -> Result<StatusCode, (StatusCode, String)> {
    let db = state.db.write();
    let deleted = db.execute("DELETE FROM s3_credentials WHERE id = ?1", rusqlite::params![&id])
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    if deleted == 0 {
        return Err((StatusCode::NOT_FOUND, "Credential not found".into()));
    }

    tracing::info!("S3 credential deleted: id={}", id);
    Ok(StatusCode::NO_CONTENT)
}
