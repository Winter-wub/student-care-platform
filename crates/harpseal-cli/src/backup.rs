//! `harpseal backup DIR` and `harpseal restore DIR [--force]`: the Core's
//! [`harpseal::backup`] over a directory on disk.
//!
//! A backup is written to `DIR.partial` and renamed to `DIR` once
//! `manifest.json` is in it, so `DIR` only ever holds a finished backup.
//! `DIR` must not exist yet, or be empty.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use harpseal::backup::{self, BackupSink, BackupSource, LoadOptions};
use harpseal::{Deployment, Error};

use crate::CliError;

/// A backup directory: entries are files under it (`tables/student.ndjson`).
#[derive(Debug, Clone)]
pub struct DirBackup {
    root: PathBuf,
}

impl DirBackup {
    /// Entries under `root`.
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    /// `root/name`, refusing names that could leave `root`.
    fn path(&self, name: &str) -> harpseal::Result<PathBuf> {
        let ok = !name.is_empty()
            && name.split('/').all(|part| {
                !part.is_empty()
                    && part != "."
                    && part != ".."
                    && part
                        .chars()
                        .all(|c| c.is_ascii_alphanumeric() || "_.-".contains(c))
            });
        if !ok {
            return Err(Error::Invalid(format!("unsafe backup entry name {name:?}")));
        }
        Ok(self.root.join(name))
    }
}

impl BackupSink for DirBackup {
    fn write(&mut self, name: &str, bytes: &[u8]) -> harpseal::Result<()> {
        let path = self.path(name)?;
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|e| io(parent, e))?;
        }
        fs::write(&path, bytes).map_err(|e| io(&path, e))
    }
}

impl BackupSource for DirBackup {
    fn read(&self, name: &str) -> harpseal::Result<Vec<u8>> {
        let path = self.path(name)?;
        fs::read(&path).map_err(|e| io(&path, e))
    }
}

fn io(path: &Path, e: std::io::Error) -> Error {
    Error::Platform(format!("{}: {e}", path.display()))
}

fn out_err(e: std::io::Error) -> CliError {
    CliError::Refused(format!("output: {e}"))
}

/// `DIR.partial`.
fn partial(dir: &Path) -> PathBuf {
    let mut p = dir.as_os_str().to_owned();
    p.push(".partial");
    PathBuf::from(p)
}

/// `backup DIR`: dumps `d` into `dir`, printing one line per table.
pub async fn backup(d: &Deployment, dir: &Path, out: &mut dyn Write) -> Result<(), CliError> {
    if dir.exists() {
        let empty = fs::read_dir(dir)
            .map_err(|e| CliError::Refused(format!("{}: {e}", dir.display())))?
            .next()
            .is_none();
        if !empty {
            return Err(CliError::Refused(format!(
                "{} is not empty; back up into a new directory",
                dir.display()
            )));
        }
    }
    let tmp = partial(dir);
    if tmp.exists() {
        return Err(CliError::Refused(format!(
            "{} exists (an unfinished backup?); remove it first",
            tmp.display()
        )));
    }
    let mut sink = DirBackup::new(&tmp);
    let report = match backup::dump(d, &mut sink).await {
        Ok(r) => r,
        Err(e) => {
            let _ = fs::remove_dir_all(&tmp);
            return Err(e.into());
        }
    };
    if dir.exists() {
        fs::remove_dir(dir).map_err(|e| CliError::Refused(format!("{}: {e}", dir.display())))?;
    }
    fs::rename(&tmp, dir)
        .map_err(|e| CliError::Refused(format!("{} -> {}: {e}", tmp.display(), dir.display())))?;
    let m = &report.manifest;
    for t in &m.tables {
        writeln!(out, "table {}: {} rows", t.name, t.rows).map_err(out_err)?;
    }
    for key in &report.missing_files {
        writeln!(
            out,
            "warning: FileStore object {key} is missing; listed as missing in the manifest"
        )
        .map_err(out_err)?;
    }
    let rows: u64 = m.tables.iter().map(|t| t.rows).sum();
    writeln!(
        out,
        "backup written to {}: {} tables, {rows} rows, {} files, {} migrations \
         (manifest sha256 {})",
        dir.display(),
        m.tables.len(),
        m.files.len() - report.missing_files.len(),
        m.migrations.len(),
        report.manifest_sha256
    )
    .map_err(out_err)?;
    writeln!(
        out,
        "it holds password hashes and session ids: keep it as safe as the database"
    )
    .map_err(out_err)
}

/// `restore DIR [--force]`: loads the backup in `dir` into `d`.
pub async fn restore(
    d: &Deployment,
    dir: &Path,
    force: bool,
    out: &mut dyn Write,
) -> Result<(), CliError> {
    let source = DirBackup::new(dir);
    let r = backup::load(d, &source, &LoadOptions::new().force(force)).await?;
    if r.wiped {
        writeln!(
            out,
            "--force: deleted the rows of every table in the backup (schema kept)"
        )
        .map_err(out_err)?;
    }
    for (t, n) in &r.tables {
        writeln!(out, "table {t}: {n} rows").map_err(out_err)?;
    }
    for key in &r.missing_files {
        writeln!(
            out,
            "warning: FileStore object {key} was missing when the backup was taken; not restored"
        )
        .map_err(out_err)?;
    }
    if !r.untouched.is_empty() {
        writeln!(
            out,
            "tables not in the backup, left untouched: {}",
            r.untouched.join(", ")
        )
        .map_err(out_err)?;
    }
    writeln!(
        out,
        "restored {} tables, {} rows, {} files from {} (manifest sha256 {}; audit backup.restore)",
        r.tables.len(),
        r.rows,
        r.files,
        dir.display(),
        r.manifest_sha256
    )
    .map_err(out_err)
}
