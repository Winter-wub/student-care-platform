//! The columns a Module's spreadsheet upload expects (#127), declared once
//! with [`ModuleRegistrar::sheet_columns`]: the help panel beside the file
//! input ([`crate::ui::upload_help`]) and the CSV template at
//! `/admin{path}/template.csv` both follow from them.

use axum::http::{HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::{MethodRouter, get};

use super::{AppState, ModuleRegistrar};

/// One column of a Module's upload file, for the help panel and the
/// template. Build it with [`UploadColumn::required`] or
/// [`UploadColumn::optional`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct UploadColumn {
    /// The header as it must appear in the file (e.g. `รหัสนักศึกษา`).
    pub header: &'static str,
    /// Catalog key of what the column holds, in Staff's words (checked by
    /// `build()`, [`crate::BuildError::UnknownText`]).
    pub label_key: &'static str,
    /// Whether every row must have a value.
    pub required: bool,
    /// A fictional example value (`68010001`, `12,500.00`); empty for none.
    pub example: &'static str,
}

impl UploadColumn {
    /// A column every row must fill.
    pub const fn required(
        header: &'static str,
        label_key: &'static str,
        example: &'static str,
    ) -> Self {
        Self {
            header,
            label_key,
            required: true,
            example,
        }
    }

    /// A column that may be empty.
    pub const fn optional(
        header: &'static str,
        label_key: &'static str,
        example: &'static str,
    ) -> Self {
        Self {
            header,
            label_key,
            required: false,
            example,
        }
    }
}

/// Where the template of the upload route at `path` (as registered, e.g.
/// `/fees/upload`) is served, under `/admin`.
pub(crate) fn template_path(path: &str) -> String {
    format!("/admin{path}/template.csv")
}

impl ModuleRegistrar {
    /// Declares the columns of the [`ModuleRegistrar::sheet_upload`] route
    /// at `path` (the same `path`, before or after `sheet_upload`).
    /// Optional: a Module that declares none still gets the file rules
    /// from [`crate::ui::upload_help`], only no column table and no
    /// template.
    ///
    /// With columns, the Core serves `GET /admin{path}/template.csv` (the
    /// header row, UTF-8 with a BOM so Excel reads Thai) to Staff holding
    /// the upload's Permission, and `ui::upload_help` lists the columns
    /// with their labels and examples. `path` has no parameters. `build()`
    /// refuses columns for a path with no `sheet_upload` in the same
    /// Module (also a path registered with `back_office` instead) or with a
    /// parameter ([`crate::BuildError::InvalidRoute`]), a template path another
    /// route already takes ([`crate::BuildError::DuplicateRoute`]) and an
    /// unknown `label_key` ([`crate::BuildError::UnknownText`]).
    pub fn sheet_columns(
        &mut self,
        path: &'static str,
        columns: &'static [UploadColumn],
    ) -> &mut Self {
        match self.uploads.iter_mut().find(|u| u.path == path) {
            Some(u) => u.columns = Some(columns),
            // A placeholder `sheet_upload` takes over; `build()` refuses
            // one still left (with the Module's name), so its 0 never
            // becomes a body limit.
            None => self.uploads.push(super::UploadSpec {
                path,
                max_body: 0,
                too_large: None,
                columns: Some(columns),
            }),
        }
        self
    }
}

/// The template route of an upload with `columns`.
pub(crate) fn template_route(
    path: &'static str,
    columns: &'static [UploadColumn],
) -> MethodRouter<AppState> {
    get(move || async move { template(path, columns) })
}

fn template(path: &str, columns: &[UploadColumn]) -> Response {
    let headers: Vec<&str> = columns.iter().map(|c| c.header).collect();
    let body = format!("\u{feff}{}", crate::sheet::csv::line(&headers));
    // `/fees/upload` → `fees-template.csv`.
    let stem = path
        .trim_start_matches('/')
        .split('/')
        .next()
        .filter(|s| s.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_'))
        .unwrap_or("upload");
    let name = format!("attachment; filename=\"{stem}-template.csv\"");
    let mut resp = (StatusCode::OK, body).into_response();
    let h = resp.headers_mut();
    h.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("text/csv; charset=utf-8"),
    );
    h.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    if let Ok(v) = HeaderValue::from_str(&name) {
        h.insert(header::CONTENT_DISPOSITION, v);
    }
    resp
}

/// The first path of one Module's declared columns that `build()` must
/// refuse: one with no [`ModuleRegistrar::sheet_upload`] (columns waiting
/// in a placeholder, or set on another kind of upload) or with a
/// parameter (its template would need one too); else `None`.
pub(crate) fn orphan_columns(uploads: &[super::UploadSpec]) -> Option<&'static str> {
    uploads
        .iter()
        .filter(|u| u.columns.is_some())
        .find(|u| u.max_body != super::upload::UPLOAD_BODY_LIMIT || u.path.contains('{'))
        .map(|u| u.path)
}
