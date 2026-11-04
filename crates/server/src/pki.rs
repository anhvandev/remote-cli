//! Server CA: issues the Agent-port server certificate and per-Device client certificates.

use rcgen::{
    BasicConstraints, Certificate, CertificateParams, CertificateSigningRequestParams, DnType,
    ExtendedKeyUsagePurpose, IsCa, KeyPair,
};
use rustls::pki_types::pem::PemObject;
use sha2::{Digest, Sha256};
use sqlx::PgPool;

use crate::vault::MasterKey;

/// AAD of the sealed CA private key.
const CA_KEY_AAD: &[u8] = b"server_ca.key";

pub struct Ca {
    pub cert_pem: String,
    /// Hex SHA-256 of the CA certificate (DER): handed out with Enrollment Codes so Agents can pin it.
    pub fingerprint: String,
    cert: Certificate,
    key: KeyPair,
}

pub struct Issued {
    pub cert_pem: String,
    pub cert_der: Vec<u8>,
    pub key_der: Vec<u8>,
}

impl Ca {
    /// Loads the CA from Postgres, creating it on first start. With a master key the private key is stored sealed
    /// (a plaintext one from before is sealed now); without one it stays plaintext.
    pub async fn load_or_create(db: &PgPool, master: Option<&MasterKey>) -> anyhow::Result<Self> {
        let key = KeyPair::generate()?;
        let mut params = CertificateParams::new(vec![])?;
        params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
        params.distinguished_name.push(DnType::CommonName, "rcli CA");
        let cert = params.self_signed(&key)?;
        let (plain, sealed) = match master {
            Some(m) => (None, Some(m.seal_bytes(CA_KEY_AAD, key.serialize_pem().as_bytes())?)),
            None => (Some(key.serialize_pem()), None),
        };
        sqlx::query("INSERT INTO server_ca (cert_pem, key_pem, key_sealed) VALUES ($1, $2, $3) ON CONFLICT DO NOTHING")
            .bind(cert.pem())
            .bind(plain)
            .bind(sealed)
            .execute(db)
            .await?;
        let (cert_pem, key_pem, key_sealed): (String, Option<String>, Option<Vec<u8>>) =
            sqlx::query_as("SELECT cert_pem, key_pem, key_sealed FROM server_ca").fetch_one(db).await?;
        let key_pem = match (key_pem, key_sealed, master) {
            (_, Some(sealed), Some(m)) => String::from_utf8(m.open_bytes(CA_KEY_AAD, &sealed)?)?,
            (_, Some(_), None) => anyhow::bail!("the CA private key is sealed: RCLI_MASTER_KEY is required"),
            (Some(plain), None, Some(m)) => {
                sqlx::query("UPDATE server_ca SET key_sealed = $1, key_pem = NULL")
                    .bind(m.seal_bytes(CA_KEY_AAD, plain.as_bytes())?)
                    .execute(db)
                    .await?;
                plain
            }
            (Some(plain), None, None) => plain,
            (None, None, _) => anyhow::bail!("server_ca has no private key"),
        };
        let key = KeyPair::from_pem(&key_pem)?;
        // Re-signing yields an equivalent issuer (same DN and key) for signing; peers trust `cert_pem`.
        let cert = CertificateParams::from_ca_cert_pem(&cert_pem)?.self_signed(&key)?;
        let der = rustls::pki_types::CertificateDer::from_pem_slice(cert_pem.as_bytes())?;
        let fingerprint = hex::encode(fingerprint(&der));
        Ok(Self { cert_pem, fingerprint, cert, key })
    }

    pub fn issue_server_cert(&self, hostnames: Vec<String>) -> anyhow::Result<Issued> {
        let key = KeyPair::generate()?;
        let mut params = CertificateParams::new(hostnames)?;
        params.extended_key_usages = vec![ExtendedKeyUsagePurpose::ServerAuth];
        let cert = params.signed_by(&key, &self.cert, &self.key)?;
        Ok(Issued { cert_pem: cert.pem(), cert_der: cert.der().to_vec(), key_der: key.serialize_der() })
    }

    /// Signs only the CSR's public key; every certificate field is chosen by the Server.
    pub fn sign_device_csr(&self, csr_pem: &str, device_id: &str) -> anyhow::Result<Issued> {
        let mut csr = CertificateSigningRequestParams::from_pem(csr_pem)?;
        let mut params = CertificateParams::default();
        params.distinguished_name.push(DnType::CommonName, device_id);
        params.extended_key_usages = vec![ExtendedKeyUsagePurpose::ClientAuth];
        csr.params = params;
        let cert = csr.signed_by(&self.cert, &self.key)?;
        Ok(Issued { cert_pem: cert.pem(), cert_der: cert.der().to_vec(), key_der: vec![] })
    }
}

pub fn fingerprint(der: &[u8]) -> Vec<u8> {
    Sha256::digest(der).to_vec()
}
