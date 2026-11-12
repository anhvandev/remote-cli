//! File storage: install packages (and, later, Agent builds) in an S3-compatible store (SeaweedFS in compose).
//! Objects are content addressed, `<prefix>/<sha256 hex>`, and streamed in and out; nothing holds a whole file.

use std::{ops::Range, time::Duration};

use axum::body::Bytes;
use futures::{stream::BoxStream, Stream, StreamExt};
use object_store::{
    aws::{AmazonS3, AmazonS3Builder},
    path::Path,
    signer::Signer,
    GetOptions, GetRange, ObjectStore, WriteMultipart,
};
use sha2::{Digest, Sha256};

/// Key prefix of install packages.
pub const PACKAGES: &str = "packages";

/// Where the store is (`RCLI_S3_*`).
#[derive(Clone, Debug)]
pub struct S3Config {
    pub endpoint: String,
    pub bucket: String,
    pub access_key: String,
    pub secret_key: String,
}

/// What `put` stored.
pub struct Stored {
    /// Lowercase hex.
    pub sha256: String,
    pub size: u64,
}

pub struct Storage {
    s3: AmazonS3,
}

impl Storage {
    /// Connects to the store and creates its bucket if missing.
    pub async fn connect(config: &S3Config) -> anyhow::Result<Self> {
        let s3 = AmazonS3Builder::new()
            .with_endpoint(&config.endpoint)
            .with_bucket_name(&config.bucket)
            .with_access_key_id(&config.access_key)
            .with_secret_access_key(&config.secret_key)
            .with_region("us-east-1")
            .with_allow_http(true)
            .build()?;
        // object_store has no CreateBucket: sign a PUT of the bucket itself.
        let url = s3.signed_url(reqwest::Method::PUT, &Path::from(""), Duration::from_secs(60)).await?;
        let resp = reqwest::Client::new().put(url).send().await?;
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        anyhow::ensure!(
            status.is_success() || body.contains("BucketAlreadyOwnedByYou") || body.contains("BucketAlreadyExists"),
            "creating the storage bucket {} at {}: {status} {body}",
            config.bucket,
            config.endpoint
        );
        Ok(Self { s3 })
    }

    /// Streams `body` into the store at `<prefix>/<its sha256>`. On any error nothing is left behind.
    pub async fn put<E>(&self, prefix: &str, body: impl Stream<Item = Result<Bytes, E>>) -> anyhow::Result<Stored>
    where
        E: Into<Box<dyn std::error::Error + Send + Sync>>,
    {
        let temp = Path::from(format!("tmp/{}", uuid::Uuid::new_v4()));
        let mut upload = WriteMultipart::new(self.s3.put_multipart(&temp).await?);
        let mut body = std::pin::pin!(body);
        let (mut hash, mut size) = (Sha256::new(), 0u64);
        let written: anyhow::Result<()> = async {
            while let Some(chunk) = body.next().await {
                let chunk = chunk.map_err(|e| anyhow::anyhow!(e.into()))?;
                hash.update(&chunk);
                size += chunk.len() as u64;
                // Bounds memory: at most a few parts are buffered or in flight.
                upload.wait_for_capacity(4).await?;
                upload.write(&chunk);
            }
            Ok(())
        }
        .await;
        if let Err(e) = written {
            let _ = upload.abort().await;
            return Err(e);
        }
        upload.finish().await?;
        let sha256 = hex::encode(hash.finalize());
        let copied = self.s3.copy(&temp, &key(prefix, &sha256)).await;
        let _ = self.s3.delete(&temp).await;
        copied?;
        Ok(Stored { sha256, size })
    }

    /// Streams `<prefix>/<sha256>`, or the byte `range` of it. A full read fails at its end, before the last
    /// chunk, when the bytes do not hash to `sha256`, so corrupted bytes are never received as a whole file.
    pub async fn get(&self, prefix: &str, sha256: &str, range: Option<Range<u64>>) -> anyhow::Result<BoxStream<'static, anyhow::Result<Bytes>>> {
        let options = GetOptions { range: range.clone().map(GetRange::Bounded), ..Default::default() };
        let stream = self.s3.get_opts(&key(prefix, sha256), options).await?.into_stream().map(|c| c.map_err(anyhow::Error::from));
        if range.is_some() {
            return Ok(stream.boxed());
        }
        let expected = sha256.to_owned();
        // Holds back one chunk, so the last one is sent only after the hash matched.
        let verified = futures::stream::unfold(
            (stream, Sha256::new(), None::<Bytes>, false),
            move |(mut stream, mut hash, mut held, done)| {
                let expected = expected.clone();
                async move {
                    if done {
                        return None;
                    }
                    loop {
                        match stream.next().await {
                            Some(Ok(chunk)) => {
                                hash.update(&chunk);
                                if let Some(prev) = held.replace(chunk) {
                                    return Some((Ok(prev), (stream, hash, held, false)));
                                }
                            }
                            Some(Err(e)) => return Some((Err(e), (stream, hash, None, true))),
                            None => {
                                let actual = hex::encode(hash.clone().finalize());
                                let last = if actual == expected {
                                    Ok(held.take()?)
                                } else {
                                    Err(anyhow::anyhow!("stored object {expected} is corrupted: its bytes hash to {actual}"))
                                };
                                return Some((last, (stream, hash, None, true)));
                            }
                        }
                    }
                }
            },
        );
        Ok(verified.boxed())
    }

    pub async fn delete(&self, prefix: &str, sha256: &str) -> anyhow::Result<()> {
        Ok(self.s3.delete(&key(prefix, sha256)).await?)
    }
}

fn key(prefix: &str, sha256: &str) -> Path {
    Path::from(format!("{prefix}/{sha256}"))
}
