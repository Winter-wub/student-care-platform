//! A Module's back-office spreadsheet upload (#74): the registrar method
//! [`ModuleRegistrar::sheet_upload`] and the extractor [`SheetUpload`].
//! What happens to the file until the Staff member confirms is in
//! [`super::kept`].

use std::collections::BTreeMap;

use axum::extract::{FromRequest, Multipart, Request};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::MethodRouter;

use super::{AppState, ModuleRegistrar, UploadSpec};
use crate::sheet::{SheetError, Workbook};

/// Largest file a Module upload accepts, in bytes: the roster import's
/// (4 MiB).
pub const MAX_UPLOAD_BYTES: usize = crate::kernel::roster::preview::MAX_FILE_BYTES;
/// Largest request body of a Module upload route: the file and the
/// form's other fields (the roster import's limit).
pub(crate) const UPLOAD_BODY_LIMIT: usize = MAX_UPLOAD_BYTES + 64 * 1024;
/// How long [`crate::Uploads::keep`] keeps a file for its confirm: 24 hours.
/// The tick deletes it after that.
pub const UPLOAD_KEEP_MS: i64 = 86_400_000;

/// The multipart part that holds the file.
const FILE_FIELD: &str = "file";

impl ModuleRegistrar {
    /// A back-office route at `/admin{path}` that takes a spreadsheet
    /// upload (`multipart/form-data`, the file in the part named `file`),
    /// guarded like [`ModuleRegistrar::back_office`]: only Staff holding
    /// `permission` reach it, and the CSRF token must be in the `_csrf`
    /// part (put [`crate::CurrentStaff::csrf_field`] before the file
    /// input) or the `X-CSRF-Token` header; otherwise 403.
    ///
    /// The body may be as large as the roster import's
    /// ([`MAX_UPLOAD_BYTES`] for the file, plus 64 KiB for the other
    /// fields) instead of the 64 KiB of other back-office forms. It is read
    /// only once the Permission is checked. A larger body is answered with
    /// the Kernel's 413 page before the handler runs.
    ///
    /// The handler extracts [`SheetUpload`] (or `Result<SheetUpload,
    /// UploadRejection>` to answer refusals with its own page), reads it
    /// with [`SheetUpload::workbook`] and keeps it for a later confirm with
    /// [`crate::Uploads::keep`]. `build()` refuses a path registered twice
    /// ([`crate::BuildError::DuplicateRoute`]).
    pub fn sheet_upload(
        &mut self,
        permission: &'static str,
        path: &'static str,
        route: MethodRouter<AppState>,
    ) -> &mut Self {
        let spec = UploadSpec {
            path,
            max_body: UPLOAD_BODY_LIMIT,
            too_large: None,
        };
        self.back_office_upload(permission, spec, route)
    }
}

/// What a file holds, told by its bytes and confirmed by its name.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum SheetKind {
    /// UTF-8 text named `.csv`.
    Csv,
    /// A ZIP container named `.xlsx`.
    Xlsx,
}

impl SheetKind {
    /// Sniffs `bytes`: an XLSX starts with the ZIP signature `PK\x03\x04`;
    /// a CSV is UTF-8 text with no NUL byte. The name must say the same
    /// (`.xlsx` or `.csv`, any case), so a renamed binary or a ZIP named
    /// `.csv` is `None`.
    pub fn sniff(bytes: &[u8], file_name: &str) -> Option<Self> {
        let name = file_name.to_ascii_lowercase();
        let zip = bytes.starts_with(b"PK\x03\x04");
        if name.ends_with(".xlsx") && zip {
            return Some(Self::Xlsx);
        }
        let text = !zip && !bytes.contains(&0) && std::str::from_utf8(bytes).is_ok();
        (name.ends_with(".csv") && text).then_some(Self::Csv)
    }

    /// The MIME type the FileStore keeps.
    pub fn content_type(self) -> &'static str {
        match self {
            Self::Csv => "text/csv",
            Self::Xlsx => "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
        }
    }

    /// `csv` or `xlsx`.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Csv => "csv",
            Self::Xlsx => "xlsx",
        }
    }

    pub(crate) fn parse(s: &str) -> Option<Self> {
        match s {
            "csv" => Some(Self::Csv),
            "xlsx" => Some(Self::Xlsx),
            _ => None,
        }
    }
}

/// Why [`SheetUpload`] refused a request. As a response: the status and
/// a short Thai text; take `Result<SheetUpload, UploadRejection>` to show
/// your own page instead.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum UploadRejection {
    /// The file is over [`MAX_UPLOAD_BYTES`] (413).
    TooLarge,
    /// No `file` part, or an empty file (400).
    NoFile,
    /// Not a CSV or XLSX by its bytes and name ([`SheetKind::sniff`], 415).
    Unsupported,
    /// Named `.csv` but not UTF-8 (a Thai CSV saved as TIS-620 or
    /// Windows-874; 415).
    NotUtf8,
    /// Not a readable `multipart/form-data` body, or two `file` parts (400).
    Malformed,
}

impl UploadRejection {
    /// The HTTP status.
    pub fn status(self) -> StatusCode {
        match self {
            Self::TooLarge => StatusCode::PAYLOAD_TOO_LARGE,
            Self::Unsupported | Self::NotUtf8 => StatusCode::UNSUPPORTED_MEDIA_TYPE,
            Self::NoFile | Self::Malformed => StatusCode::BAD_REQUEST,
        }
    }

    /// A short Thai text for Staff.
    pub fn text(self) -> &'static str {
        match self {
            Self::TooLarge => "ไฟล์ใหญ่เกิน 4 MB",
            Self::NoFile => "ไม่พบไฟล์ที่อัปโหลด",
            Self::Unsupported => "รับเฉพาะไฟล์ CSV หรือ XLSX",
            Self::NotUtf8 => "ไฟล์ CSV ต้องเป็น UTF-8 (บันทึกแบบ CSV UTF-8)",
            Self::Malformed => "อ่านแบบฟอร์มที่ส่งมาไม่ได้",
        }
    }
}

impl IntoResponse for UploadRejection {
    fn into_response(self) -> Response {
        (self.status(), self.text()).into_response()
    }
}

/// An uploaded spreadsheet, for the handler of a
/// [`ModuleRegistrar::sheet_upload`] route: the `file` part (at most
/// [`MAX_UPLOAD_BYTES`], a CSV or XLSX by [`SheetKind::sniff`]) and the
/// form's other text parts.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct SheetUpload {
    /// The file name as sent, without any directory and with control
    /// characters, `:` and `"` replaced (at most 100 characters).
    pub file_name: String,
    /// What the file holds.
    pub kind: SheetKind,
    /// The file.
    pub bytes: Vec<u8>,
    /// The other text parts by name (the last of a repeated name), without
    /// `_csrf`.
    pub fields: BTreeMap<String, String>,
}

impl SheetUpload {
    /// Reads the file with the roster import's reader and limits
    /// ([`crate::sheet`]).
    pub fn workbook(&self) -> Result<Workbook, SheetError> {
        Workbook::open(&self.bytes, &self.file_name)
    }

    /// Reads a request body; see [`UploadRejection`] for the refusals.
    async fn read(mut m: Multipart) -> Result<Self, UploadRejection> {
        let mut file: Option<(String, Vec<u8>)> = None;
        let mut fields = BTreeMap::new();
        while let Some(mut field) = m.next_field().await.map_err(rejection)? {
            let name = field.name().unwrap_or_default().to_string();
            if name == FILE_FIELD {
                if file.is_some() {
                    return Err(UploadRejection::Malformed);
                }
                let file_name = field.file_name().unwrap_or_default().to_string();
                let mut bytes = Vec::new();
                while let Some(chunk) = field.chunk().await.map_err(rejection)? {
                    if bytes.len() + chunk.len() > MAX_UPLOAD_BYTES {
                        return Err(UploadRejection::TooLarge);
                    }
                    bytes.extend_from_slice(&chunk);
                }
                file = Some((file_name, bytes));
            } else {
                let text = field.text().await.map_err(rejection)?;
                if name != crate::auth::CSRF_FIELD {
                    fields.insert(name, text);
                }
            }
        }
        let Some((name, bytes)) = file.filter(|(_, b)| !b.is_empty()) else {
            return Err(UploadRejection::NoFile);
        };
        let file_name = crate::kernel::roster::file::safe_name(&name);
        let kind = SheetKind::sniff(&bytes, &file_name).ok_or_else(|| {
            let csv = file_name.to_ascii_lowercase().ends_with(".csv");
            if csv && !bytes.starts_with(b"PK\x03\x04") && std::str::from_utf8(&bytes).is_err() {
                UploadRejection::NotUtf8
            } else {
                UploadRejection::Unsupported
            }
        })?;
        Ok(Self {
            file_name,
            kind,
            bytes,
            fields,
        })
    }
}

impl<S: Send + Sync> FromRequest<S> for SheetUpload {
    type Rejection = UploadRejection;

    async fn from_request(req: Request, state: &S) -> Result<Self, Self::Rejection> {
        // A body sent with the CSRF header was not read by the guard: its
        // declared length is checked here, and axum's limit while reading.
        let declared = req
            .headers()
            .get(axum::http::header::CONTENT_LENGTH)
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.parse::<u64>().ok());
        if declared.is_some_and(|n| n > UPLOAD_BODY_LIMIT as u64) {
            return Err(UploadRejection::TooLarge);
        }
        let m = Multipart::from_request(req, state)
            .await
            .map_err(|_| UploadRejection::Malformed)?;
        Self::read(m).await
    }
}

/// A body over axum's limit is 413; any other reading error is malformed.
fn rejection(e: axum::extract::multipart::MultipartError) -> UploadRejection {
    if e.status() == StatusCode::PAYLOAD_TOO_LARGE {
        UploadRejection::TooLarge
    } else {
        UploadRejection::Malformed
    }
}

#[cfg(test)]
mod tests {
    use super::SheetKind;

    #[test]
    fn the_kind_comes_from_the_bytes_and_the_name_must_agree() {
        let zip = b"PK\x03\x04rest of a zip";
        assert_eq!(SheetKind::sniff(zip, "fees.XLSX"), Some(SheetKind::Xlsx));
        assert_eq!(
            SheetKind::sniff("รหัส,ยอด\n1,2\n".as_bytes(), "fees.csv"),
            Some(SheetKind::Csv)
        );
        assert_eq!(SheetKind::sniff(zip, "fees.csv"), None, "a zip named csv");
        assert_eq!(
            SheetKind::sniff(b"a,b\n", "fees.xlsx"),
            None,
            "text named xlsx"
        );
        assert_eq!(SheetKind::sniff(b"a,b\n", "fees.txt"), None);
        assert_eq!(
            SheetKind::sniff(b"\xff\xfe\x00a", "fees.csv"),
            None,
            "UTF-16"
        );
        assert_eq!(SheetKind::sniff(b"MZ\x90\x00", "fees.csv"), None, "binary");
        for k in [SheetKind::Csv, SheetKind::Xlsx] {
            assert_eq!(SheetKind::parse(k.as_str()), Some(k));
        }
    }
}
