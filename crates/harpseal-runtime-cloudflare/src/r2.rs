//! [`R2Files`]: an R2 bucket binding as the `FileStore`.

use async_trait::async_trait;
use harpseal::{Error, FileStore, Result, check_file_key};
use worker::send::SendWrapper;
use worker::{Bucket, Env, HttpMetadata};

/// An R2 bucket binding (`[[r2_buckets]] binding` in `wrangler.toml`) as
/// the FileStore. When the Worker has no such binding every call answers
/// [`Error::Platform`] naming it, so a Deployment without files still
/// boots.
pub struct R2Files {
    // `Bucket` holds a JS object (`!Send`); a Worker isolate is one thread.
    bucket: Option<SendWrapper<Bucket>>,
    binding: String,
}

impl R2Files {
    /// The bucket bound as `binding`, if the Worker has it.
    pub fn from_env(env: &Env, binding: &str) -> Self {
        Self {
            bucket: env.bucket(binding).ok().map(SendWrapper),
            binding: binding.to_string(),
        }
    }

    fn bucket(&self) -> Result<&Bucket> {
        self.bucket.as_ref().map(|b| &b.0).ok_or_else(|| {
            Error::Platform(format!(
                "no R2 binding `{}`: add [[r2_buckets]] to wrangler.toml",
                self.binding
            ))
        })
    }
}

fn platform(e: impl core::fmt::Display) -> Error {
    Error::Platform(e.to_string())
}

#[async_trait]
impl FileStore for R2Files {
    async fn put(&self, key: &str, bytes: Vec<u8>, content_type: &str) -> Result<()> {
        check_file_key(key)?;
        let bucket = self.bucket()?;
        let meta = HttpMetadata {
            content_type: Some(content_type.to_string()),
            ..HttpMetadata::default()
        };
        worker::send::SendFuture::new(async move {
            bucket
                .put(key, bytes)
                .http_metadata(meta)
                .execute()
                .await
                .map(|_| ())
                .map_err(platform)
        })
        .await
    }

    async fn get(&self, key: &str) -> Result<Option<Vec<u8>>> {
        check_file_key(key)?;
        let bucket = self.bucket()?;
        worker::send::SendFuture::new(async move {
            let Some(obj) = bucket.get(key).execute().await.map_err(platform)? else {
                return Ok(None);
            };
            match obj.body() {
                Some(body) => body.bytes().await.map(Some).map_err(platform),
                None => Ok(Some(Vec::new())),
            }
        })
        .await
    }

    async fn delete(&self, key: &str) -> Result<()> {
        check_file_key(key)?;
        let bucket = self.bucket()?;
        worker::send::SendFuture::new(async move { bucket.delete(key).await.map_err(platform) })
            .await
    }
}
