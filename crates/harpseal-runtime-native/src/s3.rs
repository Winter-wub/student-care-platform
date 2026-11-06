//! [`S3Files`]: S3-compatible object storage (AWS S3, Cloudflare R2's S3
//! API, MinIO, ...) as the `FileStore`. Requests go through the
//! [`HttpClient`] port and are signed with AWS Signature V4 by hand (HMAC
//! and SHA-256 from `hmac` / `sha2`), so no AWS SDK is pulled in.
//! Path-style URLs: `{endpoint}/{bucket}/{key}`.

use std::sync::Arc;

use async_trait::async_trait;
use harpseal::{Clock, Error, FileStore, HttpClient, Result, check_file_key};
use hmac::{Hmac, KeyInit, Mac};
use sha2::{Digest, Sha256};

/// Where the bucket is and who may write to it. The keys are secrets: they
/// come from the environment only.
#[derive(Clone)]
#[non_exhaustive]
pub struct S3Settings {
    /// `https://<account>.r2.cloudflarestorage.com`, `https://s3.<region>.amazonaws.com`, ...
    pub endpoint: String,
    /// Bucket name.
    pub bucket: String,
    /// Signing region (`auto` for R2, `us-east-1` for MinIO's default).
    pub region: String,
    /// Access key id.
    pub access_key_id: String,
    /// Secret access key.
    pub secret_access_key: String,
}

impl S3Settings {
    /// Settings for one bucket.
    pub fn new(
        endpoint: impl Into<String>,
        bucket: impl Into<String>,
        region: impl Into<String>,
        access_key_id: impl Into<String>,
        secret_access_key: impl Into<String>,
    ) -> Self {
        Self {
            endpoint: endpoint.into().trim_end_matches('/').to_string(),
            bucket: bucket.into(),
            region: region.into(),
            access_key_id: access_key_id.into(),
            secret_access_key: secret_access_key.into(),
        }
    }
}

impl core::fmt::Debug for S3Settings {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("S3Settings")
            .field("endpoint", &self.endpoint)
            .field("bucket", &self.bucket)
            .field("region", &self.region)
            .field("access_key_id", &"<redacted>")
            .field("secret_access_key", &"<redacted>")
            .finish()
    }
}

/// An S3-compatible bucket as the FileStore.
pub struct S3Files {
    s: S3Settings,
    http: Arc<dyn HttpClient>,
    clock: Arc<dyn Clock>,
}

impl S3Files {
    /// Signs with `clock`'s time and sends through `http`. The endpoint
    /// must be a bare origin (`https://host[:port]`): a path prefix or a
    /// query is refused, because path-style URLs put the bucket first.
    pub fn new(s: S3Settings, http: Arc<dyn HttpClient>, clock: Arc<dyn Clock>) -> Result<Self> {
        let bad = |why: &str| Error::Invalid(format!("S3 endpoint {:?}: {why}", s.endpoint));
        let uri: http::Uri = s.endpoint.parse().map_err(|_| bad("not a URL"))?;
        if !matches!(uri.scheme_str(), Some("https" | "http")) || uri.host().is_none() {
            return Err(bad("expected http(s)://host[:port]"));
        }
        if !matches!(uri.path(), "" | "/") || uri.query().is_some() {
            return Err(bad("a path prefix or query is not supported"));
        }
        Ok(Self { s, http, clock })
    }

    fn request(
        &self,
        method: http::Method,
        key: &str,
        body: Vec<u8>,
        content_type: Option<&str>,
    ) -> Result<http::Request<Vec<u8>>> {
        check_file_key(key)?;
        let path = format!("/{}/{}", uri_encode(&self.s.bucket), uri_encode(key));
        let url = format!("{}{path}", self.s.endpoint);
        let uri: http::Uri = url.parse().map_err(platform)?;
        let host = host_of(&uri)?;
        let amz_date = amz_date(self.clock.now().ms());
        let payload_hash = hex(&Sha256::digest(&body));
        let auth = sign(&SignInput {
            s: &self.s,
            method: method.as_str(),
            path: &path,
            amz_date: &amz_date,
            payload_hash: &payload_hash,
            headers: &[
                ("host", &host),
                ("x-amz-content-sha256", &payload_hash),
                ("x-amz-date", &amz_date),
            ],
        });
        let mut b = http::Request::builder()
            .method(method)
            .uri(uri)
            .header("x-amz-date", &amz_date)
            .header("x-amz-content-sha256", &payload_hash)
            .header(http::header::AUTHORIZATION, auth);
        if let Some(ct) = content_type {
            b = b.header(http::header::CONTENT_TYPE, ct);
        }
        b.body(body).map_err(platform)
    }

    async fn call(&self, req: http::Request<Vec<u8>>) -> Result<http::Response<Vec<u8>>> {
        self.http.send(req).await
    }
}

fn platform(e: impl core::fmt::Display) -> Error {
    Error::Platform(e.to_string())
}

fn status_err(op: &str, key: &str, resp: &http::Response<Vec<u8>>) -> Error {
    let body = String::from_utf8_lossy(resp.body());
    Error::Platform(format!(
        "s3 {op} {key:?}: {} {}",
        resp.status().as_u16(),
        body.chars().take(300).collect::<String>()
    ))
}

#[async_trait]
impl FileStore for S3Files {
    async fn put(&self, key: &str, bytes: Vec<u8>, content_type: &str) -> Result<()> {
        let req = self.request(http::Method::PUT, key, bytes, Some(content_type))?;
        let resp = self.call(req).await?;
        if resp.status().is_success() {
            Ok(())
        } else {
            Err(status_err("put", key, &resp))
        }
    }

    async fn get(&self, key: &str) -> Result<Option<Vec<u8>>> {
        let req = self.request(http::Method::GET, key, Vec::new(), None)?;
        let resp = self.call(req).await?;
        match resp.status().as_u16() {
            200..=299 => Ok(Some(resp.into_body())),
            404 => Ok(None),
            _ => Err(status_err("get", key, &resp)),
        }
    }

    async fn delete(&self, key: &str) -> Result<()> {
        let req = self.request(http::Method::DELETE, key, Vec::new(), None)?;
        let resp = self.call(req).await?;
        if resp.status().is_success() || resp.status().as_u16() == 404 {
            Ok(())
        } else {
            Err(status_err("delete", key, &resp))
        }
    }
}

/// `host[:port]`, without the scheme's default port.
fn host_of(uri: &http::Uri) -> Result<String> {
    let host = uri
        .host()
        .ok_or_else(|| platform("s3 endpoint has no host"))?;
    let default = matches!(
        (uri.scheme_str(), uri.port_u16()),
        (Some("https"), Some(443)) | (Some("http"), Some(80))
    );
    Ok(match uri.port_u16() {
        Some(p) if !default => format!("{host}:{p}"),
        _ => host.to_string(),
    })
}

struct SignInput<'a> {
    s: &'a S3Settings,
    method: &'a str,
    /// Already URI-encoded.
    path: &'a str,
    amz_date: &'a str,
    payload_hash: &'a str,
    /// Signed headers, lowercase names; `host`, `x-amz-content-sha256` and
    /// `x-amz-date` at least.
    headers: &'a [(&'a str, &'a str)],
}

/// The `Authorization` header value (SigV4, no query string).
fn sign(i: &SignInput<'_>) -> String {
    let mut headers = i.headers.to_vec();
    headers.sort_by_key(|(k, _)| *k);
    let canonical_headers: String = headers
        .iter()
        .map(|(k, v)| format!("{k}:{}\n", v.trim()))
        .collect();
    let signed_headers = headers
        .iter()
        .map(|(k, _)| *k)
        .collect::<Vec<_>>()
        .join(";");
    let canonical = format!(
        "{}\n{}\n\n{canonical_headers}\n{signed_headers}\n{}",
        i.method, i.path, i.payload_hash
    );
    let date = &i.amz_date[..8];
    let scope = format!("{date}/{}/s3/aws4_request", i.s.region);
    let to_sign = format!(
        "AWS4-HMAC-SHA256\n{}\n{scope}\n{}",
        i.amz_date,
        hex(&Sha256::digest(canonical.as_bytes()))
    );
    let key = signing_key(&i.s.secret_access_key, date, &i.s.region, "s3");
    format!(
        "AWS4-HMAC-SHA256 Credential={}/{scope}, SignedHeaders={signed_headers}, Signature={}",
        i.s.access_key_id,
        hex(&hmac(&key, to_sign.as_bytes()))
    )
}

fn hmac(key: &[u8], msg: &[u8]) -> Vec<u8> {
    let mut m = <Hmac<Sha256> as KeyInit>::new_from_slice(key).expect("HMAC takes any key length");
    m.update(msg);
    m.finalize().into_bytes().to_vec()
}

/// `kSigning` = HMAC chain over date, region, service, `aws4_request`.
fn signing_key(secret: &str, date: &str, region: &str, service: &str) -> Vec<u8> {
    let k = hmac(format!("AWS4{secret}").as_bytes(), date.as_bytes());
    let k = hmac(&k, region.as_bytes());
    let k = hmac(&k, service.as_bytes());
    hmac(&k, b"aws4_request")
}

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

/// RFC 3986 encoding of each path segment; `/` between segments is kept.
fn uri_encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' | b'/' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

/// `YYYYMMDDTHHMMSSZ` from Unix milliseconds (proleptic Gregorian, UTC).
fn amz_date(ms: i64) -> String {
    let secs = ms.div_euclid(1000);
    let (days, sod) = (secs.div_euclid(86_400), secs.rem_euclid(86_400));
    // Howard Hinnant's civil_from_days.
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!(
        "{y:04}{m:02}{d:02}T{:02}{:02}{:02}Z",
        sod / 3600,
        sod % 3600 / 60,
        sod % 60
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn signing_key_matches_aws_example() {
        // https://docs.aws.amazon.com/IAM/latest/UserGuide/reference_sigv-create-signed-request.html
        let k = signing_key(
            "wJalrXUtnFEMI/K7MDENG+bPxRfiCYEXAMPLEKEY",
            "20120215",
            "us-east-1",
            "iam",
        );
        assert_eq!(
            hex(&k),
            "f4780e2d9f65fa895f9c67b32ce1baf0b0d8a43505a000a1a9e090d414db404d"
        );
    }

    /// "GET Object" from the SigV4 S3 examples:
    /// https://docs.aws.amazon.com/AmazonS3/latest/API/sig-v4-header-based-auth.html
    #[test]
    fn signature_matches_aws_get_object_example() {
        let s = S3Settings::new(
            "https://examplebucket.s3.amazonaws.com",
            "examplebucket",
            "us-east-1",
            "AKIAIOSFODNN7EXAMPLE",
            "wJalrXUtnFEMI/K7MDENG/bPxRfiCYEXAMPLEKEY",
        );
        let empty = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";
        let auth = sign(&SignInput {
            s: &s,
            method: "GET",
            path: "/test.txt",
            amz_date: "20130524T000000Z",
            payload_hash: empty,
            headers: &[
                ("x-amz-date", "20130524T000000Z"),
                ("range", "bytes=0-9"),
                ("host", "examplebucket.s3.amazonaws.com"),
                ("x-amz-content-sha256", empty),
            ],
        });
        assert_eq!(
            auth,
            "AWS4-HMAC-SHA256 Credential=AKIAIOSFODNN7EXAMPLE/20130524/us-east-1/s3/aws4_request, \
             SignedHeaders=host;range;x-amz-content-sha256;x-amz-date, \
             Signature=f0e8bdb87c964420e857bd35b5d6ed310bd44f0170aba48dd91039c6036bdb41"
        );
    }

    #[test]
    fn formats_amz_date() {
        assert_eq!(amz_date(0), "19700101T000000Z");
        assert_eq!(amz_date(1_330_000_000_000), "20120223T122640Z");
        assert_eq!(amz_date(1_709_164_800_000), "20240229T000000Z");
    }

    #[test]
    fn encodes_keys() {
        assert_eq!(uri_encode("a b/ค.txt"), "a%20b/%E0%B8%84.txt");
    }
}
