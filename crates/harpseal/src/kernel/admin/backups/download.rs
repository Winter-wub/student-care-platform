//! `GET /admin/backups/{id}/download`: one finished backup as a `.tar`
//! (POSIX ustar, no compression), streamed object by object: only one
//! FileStore object is in memory at a time, so a backup larger than the
//! Worker's memory still downloads. It unpacks to `<UTC time>/`, the
//! directory `harpseal restore` reads. Audited `backup.download` before
//! the first byte.

use std::collections::VecDeque;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};

use axum::body::{Body, Bytes};
use axum::extract::{Path, State};
use axum::http::{HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use http_body::Frame;
use serde_json::json;

use super::store;
use crate::audit::{self, Actor, RequestCtx, Target};
use crate::backup::{FileStoreBackup, MANIFEST, Manifest, file_entry, table_entry};
use crate::kernel::admin::view::View;
use crate::{AppState, CurrentStaff, Error, FileStore, Result};

/// A tar block.
const BLOCK: usize = 512;

pub(super) async fn tar(
    State(s): State<AppState>,
    staff: CurrentStaff,
    Path(id): Path<String>,
    ctx: RequestCtx,
) -> Response {
    let v = View::new(&s, &staff);
    let run = match store::get(&s.services, &id).await {
        Ok(Some(r)) if r.state == "done" => r,
        Ok(_) => return v.not_found("/admin/backups"),
        Err(e) => return v.error(&e),
    };
    let backup = FileStoreBackup::new(s.services.files.clone(), run.prefix.clone());
    let started = async {
        let (m, _) = Manifest::read(&backup).await?;
        let target = Target::new("backup_run", id.as_str());
        let after = json!({ "prefix": run.prefix, "bytes": run.bytes });
        let actor = Actor::staff(&staff);
        audit::record(
            &s.services,
            &actor,
            "backup.download",
            Some(&target),
            None,
            Some(after),
            &ctx,
        )
        .await?;
        Ok::<_, Error>(m)
    };
    let m = match started.await {
        Ok(m) => m,
        Err(e) => return v.error(&e),
    };
    let mut names: VecDeque<String> = VecDeque::from([MANIFEST.to_string()]);
    names.extend(m.tables.iter().map(|t| table_entry(&t.name)));
    names.extend(
        m.files
            .iter()
            .filter(|f| !f.missing)
            .map(|f| file_entry(&f.key)),
    );
    let dir = run
        .prefix
        .trim_end_matches('/')
        .rsplit('/')
        .next()
        .unwrap_or("backup")
        .to_string();
    let body = TarBody {
        files: s.services.files.clone(),
        prefix: run.prefix.clone(),
        dir: dir.clone(),
        mtime: m.taken_at.div_euclid(1000).max(0) as u64,
        names,
        pending: None,
        ended: false,
    };
    let mut resp = (StatusCode::OK, Body::new(body)).into_response();
    let h = resp.headers_mut();
    h.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/x-tar"),
    );
    h.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    let disposition = format!("attachment; filename=\"backup-{dir}.tar\"");
    if let Ok(d) = HeaderValue::from_str(&disposition) {
        h.insert(header::CONTENT_DISPOSITION, d);
    }
    resp
}

type Pending = Pin<Box<dyn Future<Output = (String, Result<Option<Vec<u8>>>)> + Send>>;

/// The tar, produced as the client reads it.
struct TarBody {
    files: Arc<dyn FileStore>,
    prefix: String,
    /// Top directory inside the archive.
    dir: String,
    mtime: u64,
    names: VecDeque<String>,
    pending: Option<Pending>,
    ended: bool,
}

impl http_body::Body for TarBody {
    type Data = Bytes;
    type Error = Error;

    fn poll_frame(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<core::result::Result<Frame<Bytes>, Error>>> {
        let this = self.get_mut();
        loop {
            if let Some(fut) = this.pending.as_mut() {
                let (name, got) = match fut.as_mut().poll(cx) {
                    Poll::Pending => return Poll::Pending,
                    Poll::Ready(r) => r,
                };
                this.pending = None;
                let chunk = match got {
                    Ok(Some(bytes)) => entry(&format!("{}/{name}", this.dir), &bytes, this.mtime),
                    Ok(None) => Err(Error::Invalid(format!("backup entry {name} is missing"))),
                    Err(e) => Err(e),
                };
                return Poll::Ready(Some(chunk.map(|c| Frame::data(Bytes::from(c)))));
            }
            let Some(name) = this.names.pop_front() else {
                if this.ended {
                    return Poll::Ready(None);
                }
                this.ended = true;
                // Two zero blocks end the archive.
                let end = Bytes::from(vec![0u8; 2 * BLOCK]);
                return Poll::Ready(Some(Ok(Frame::data(end))));
            };
            let files = this.files.clone();
            let key = format!("{}{name}", this.prefix);
            this.pending = Some(Box::pin(async move {
                let got = files.get(&key).await;
                (name, got)
            }));
        }
    }
}

/// One file entry: a ustar header, then `bytes` padded to a block.
fn entry(path: &str, bytes: &[u8], mtime: u64) -> Result<Vec<u8>> {
    let mut out = header(path, bytes.len() as u64, mtime)?;
    out.extend_from_slice(bytes);
    out.resize(out.len().div_ceil(BLOCK) * BLOCK, 0);
    Ok(out)
}

/// A ustar header block for a regular file at `path`.
fn header(path: &str, size: u64, mtime: u64) -> Result<Vec<u8>> {
    let (prefix, name) =
        split_path(path).ok_or_else(|| Error::Invalid(format!("tar path too long: {path}")))?;
    let mut h = vec![0u8; BLOCK];
    let put = |h: &mut Vec<u8>, at: usize, s: &[u8]| h[at..at + s.len()].copy_from_slice(s);
    put(&mut h, 0, name.as_bytes());
    put(&mut h, 100, b"0000644\0");
    put(&mut h, 108, b"0000000\0");
    put(&mut h, 116, b"0000000\0");
    put(&mut h, 124, format!("{size:011o}\0").as_bytes());
    put(&mut h, 136, format!("{mtime:011o}\0").as_bytes());
    put(&mut h, 148, b"        ");
    h[156] = b'0';
    put(&mut h, 257, b"ustar\0");
    put(&mut h, 263, b"00");
    put(&mut h, 345, prefix.as_bytes());
    let sum: u32 = h.iter().map(|&b| u32::from(b)).sum();
    put(&mut h, 148, format!("{sum:06o}\0 ").as_bytes());
    Ok(h)
}

/// `(prefix, name)` with `name` at most 100 bytes and `prefix` at most
/// 155, split at a `/`; `None` when the path cannot fit.
fn split_path(path: &str) -> Option<(&str, &str)> {
    if path.len() <= 100 {
        return Some(("", path));
    }
    path.match_indices('/')
        .map(|(i, _)| (&path[..i], &path[i + 1..]))
        .find(|(p, n)| p.len() <= 155 && n.len() <= 100 && !n.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn headers_are_ustar() {
        let e = entry("20261107T190000000Z/manifest.json", b"{}", 1_700_000_000).unwrap();
        assert_eq!(e.len(), 2 * BLOCK);
        assert_eq!(&e[..33], b"20261107T190000000Z/manifest.json");
        assert_eq!(e[33], 0);
        assert_eq!(&e[124..136], b"00000000002\0");
        assert_eq!(&e[257..263], b"ustar\0");
        // The checksum counts its own field as spaces.
        let mut h = e[..BLOCK].to_vec();
        let stored = u32::from_str_radix(std::str::from_utf8(&h[148..154]).unwrap(), 8).unwrap();
        h[148..156].copy_from_slice(b"        ");
        assert_eq!(stored, h.iter().map(|&b| u32::from(b)).sum::<u32>());
        assert_eq!(&e[BLOCK..BLOCK + 2], b"{}");
        assert!(e[BLOCK + 2..].iter().all(|&b| b == 0));
    }

    #[test]
    fn long_paths_use_the_prefix_field() {
        let long = format!("20261107T190000000Z/tables/{}.ndjson", "t".repeat(90));
        let (p, n) = split_path(&long).unwrap();
        assert_eq!(p, "20261107T190000000Z/tables");
        assert_eq!(n.len(), 97);
        assert!(split_path(&"x".repeat(300)).is_none());
    }
}
