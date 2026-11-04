//! Enrollment & Credentials: single-use Enrollment Codes exchanged for a Device certificate.

use rcli_protocol::agent::{EnrollRequest, EnrollResponse};
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::{pki, AppState};

pub enum EnrollError {
    InvalidCode,
    BadRequest(String),
    Internal(anyhow::Error),
}

impl<E: Into<anyhow::Error>> From<E> for EnrollError {
    fn from(e: E) -> Self {
        Self::Internal(e.into())
    }
}

fn hash(code: &str) -> Vec<u8> {
    Sha256::digest(code.trim().as_bytes()).to_vec()
}

/// Creates a new Enrollment Code. Only its hash is stored.
pub async fn create_code(state: &AppState) -> anyhow::Result<String> {
    let code = Uuid::new_v4().simple().to_string();
    sqlx::query("INSERT INTO enrollment_codes (code_hash, created_at) VALUES ($1, $2)")
        .bind(hash(&code))
        .bind(state.clock.now())
        .execute(&state.db)
        .await?;
    Ok(code)
}

pub async fn enroll(state: &AppState, req: EnrollRequest) -> Result<EnrollResponse, EnrollError> {
    let name = req.hostname.trim();
    if name.is_empty() || name.len() > 255 {
        return Err(EnrollError::BadRequest("hostname must be 1-255 characters".into()));
    }
    let device_id = Uuid::new_v4();
    let issued = state
        .ca
        .sign_device_csr(&req.csr_pem, &device_id.to_string())
        .map_err(|e| EnrollError::BadRequest(format!("invalid CSR: {e}")))?;

    let mut tx = state.db.begin().await?;
    let now = state.clock.now();
    // Claim the code first so concurrent use of the same code yields exactly one Device.
    let claimed = sqlx::query("UPDATE enrollment_codes SET used_at = $2 WHERE code_hash = $1 AND used_at IS NULL")
        .bind(hash(&req.code))
        .bind(now)
        .execute(&mut *tx)
        .await?
        .rows_affected();
    if claimed == 0 {
        return Err(EnrollError::InvalidCode);
    }
    sqlx::query("INSERT INTO devices (id, name, cert_fingerprint, enrolled_at) VALUES ($1, $2, $3, $4)")
        .bind(device_id)
        .bind(name)
        .bind(pki::fingerprint(&issued.cert_der))
        .bind(now)
        .execute(&mut *tx)
        .await?;
    sqlx::query("UPDATE enrollment_codes SET device_id = $2 WHERE code_hash = $1")
        .bind(hash(&req.code))
        .bind(device_id)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(EnrollResponse { device_id: device_id.to_string(), cert_pem: issued.cert_pem })
}
