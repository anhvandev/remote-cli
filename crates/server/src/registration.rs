//! Registration: a Device presents the Server Key's secret and gets a Device certificate, a 9-digit Device ID
//! and the update-signing public key to pin.

use rcli_protocol::agent::{RegisterRequest, RegisterResponse};
use uuid::Uuid;

use crate::{pki, server_key, AppState};

pub enum RegisterError {
    InvalidKey,
    BadRequest(String),
    Internal(anyhow::Error),
}

impl<E: Into<anyhow::Error>> From<E> for RegisterError {
    fn from(e: E) -> Self {
        Self::Internal(e.into())
    }
}

pub async fn register(state: &AppState, req: RegisterRequest) -> Result<RegisterResponse, RegisterError> {
    if !server_key::admits(state, &req.server_key).await {
        return Err(RegisterError::InvalidKey);
    }
    // No CSR: a reinstall only checking its Server Key before it touches the existing install.
    if req.csr_pem.is_empty() {
        return Ok(RegisterResponse { ca_pem: state.ca.cert_pem.clone(), ..Default::default() });
    }
    let name = req.hostname.trim();
    if name.is_empty() || name.len() > 255 {
        return Err(RegisterError::BadRequest("hostname must be 1-255 characters".into()));
    }
    let device_id = Uuid::new_v4();
    let issued = state
        .ca
        .sign_device_csr(&req.csr_pem, &device_id.to_string())
        .map_err(|e| RegisterError::BadRequest(format!("invalid CSR: {e}")))?;

    let device_password = rcli_protocol::password::generate();
    let password_hash = rcli_protocol::password::hash(&device_password);

    let mut tx = state.db.begin().await?;
    let device_number = next_device_number(&mut tx).await?;
    sqlx::query(
        "INSERT INTO devices (id, device_number, name, cert_fingerprint, enrolled_at, device_password_hash)
         VALUES ($1, $2, $3, $4, $5, $6)",
    )
    .bind(device_id)
    .bind(&device_number)
    .bind(name)
    .bind(pki::fingerprint(&issued.cert_der))
    .bind(state.clock.now())
    .bind(&password_hash)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(RegisterResponse {
        device_id: device_number,
        cert_pem: issued.cert_pem,
        ca_pem: state.ca.cert_pem.clone(),
        device_password,
        api_address: state.settings.api_address().await,
    })
}

/// A random free 9-digit number (100000000..=999999999).
async fn next_device_number(tx: &mut sqlx::Transaction<'_, sqlx::Postgres>) -> anyhow::Result<String> {
    use ring::rand::{SecureRandom, SystemRandom};
    let random = SystemRandom::new();
    loop {
        let mut bytes = [0u8; 8];
        random.fill(&mut bytes).map_err(|_| anyhow::anyhow!("cannot generate a Device ID"))?;
        let n = 100_000_000 + u64::from_be_bytes(bytes) % 900_000_000;
        let taken: bool = sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM devices WHERE device_number = $1)")
            .bind(n.to_string())
            .fetch_one(&mut **tx)
            .await?;
        if !taken {
            return Ok(n.to_string());
        }
    }
}
