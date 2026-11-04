//! Server CA: issues the Agent-port server certificate and per-Device client certificates.

use rcgen::{
    BasicConstraints, Certificate, CertificateParams, CertificateSigningRequestParams, DnType,
    ExtendedKeyUsagePurpose, IsCa, KeyPair,
};
use sha2::{Digest, Sha256};
use sqlx::PgPool;

pub struct Ca {
    pub cert_pem: String,
    cert: Certificate,
    key: KeyPair,
}

pub struct Issued {
    pub cert_pem: String,
    pub cert_der: Vec<u8>,
    pub key_der: Vec<u8>,
}

impl Ca {
    /// Loads the CA from Postgres, creating it on first start.
    // shortcut: CA key is stored unencrypted in Postgres; wrap it with the master key once the Secrets Vault (ticket 15) exists.
    pub async fn load_or_create(db: &PgPool) -> anyhow::Result<Self> {
        let key = KeyPair::generate()?;
        let mut params = CertificateParams::new(vec![])?;
        params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
        params.distinguished_name.push(DnType::CommonName, "rcli CA");
        let cert = params.self_signed(&key)?;
        sqlx::query("INSERT INTO server_ca (cert_pem, key_pem) VALUES ($1, $2) ON CONFLICT DO NOTHING")
            .bind(cert.pem())
            .bind(key.serialize_pem())
            .execute(db)
            .await?;
        let (cert_pem, key_pem): (String, String) =
            sqlx::query_as("SELECT cert_pem, key_pem FROM server_ca").fetch_one(db).await?;
        let key = KeyPair::from_pem(&key_pem)?;
        // Re-signing yields an equivalent issuer (same DN and key) for signing; peers trust `cert_pem`.
        let cert = CertificateParams::from_ca_cert_pem(&cert_pem)?.self_signed(&key)?;
        Ok(Self { cert_pem, cert, key })
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
