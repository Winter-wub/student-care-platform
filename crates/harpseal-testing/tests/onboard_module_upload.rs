//! A Module's upload help (#127): the columns declared once with
//! `ModuleRegistrar::sheet_columns` give `ui::upload_help` its table and a
//! CSV template behind the upload's Permission; a Module that declares
//! none still gets the file rules; `build()` refuses columns with no
//! upload, a taken template path and an unknown label.

use axum::response::Html;
use axum::routing::{get, post};
use harpseal::{
    AppState, BuildError, CatalogEntry, CurrentStaff, DeploymentConfig, Module, ModuleRegistrar,
    Permission, UploadColumn,
};
use harpseal_testing::{Browser, Harness, TestApp};

const PASSWORD: &str = "kelp forest at dawn";

const CONFIG: &str = r#"
name = "Seal Bay University (grades)"
default_locale = "th"
utc_offset = "+07:00"

[roles]
Lecturer = ["grades.import", "kernel.student.view"]
Viewer = ["kernel.student.view"]
"#;

const COLUMNS: &[UploadColumn] = &[
    UploadColumn::required("รหัสนักศึกษา", "grades.col.code", "68010001"),
    UploadColumn::required("รหัสวิชา", "grades.col.course", "SB101"),
    UploadColumn::optional("หมายเหตุ", "grades.col.note", ""),
];

const TEXTS: &[CatalogEntry] = &[
    CatalogEntry::new("grades.title", "th", "ผลการเรียน"),
    CatalogEntry::new("grades.col.code", "th", "รหัสนักศึกษา 8 หลัก"),
    CatalogEntry::new("grades.col.course", "th", "รหัสวิชา"),
    CatalogEntry::new("grades.col.note", "th", "หมายเหตุถึงนักศึกษา"),
];

/// Which way a test Module registers its upload.
#[derive(Clone, Copy)]
enum Way {
    /// `sheet_columns` after `sheet_upload`.
    After,
    /// `sheet_columns` before `sheet_upload`.
    Before,
    /// No columns: an existing Module.
    None,
    /// Columns for a path with no upload.
    Orphan,
    /// A label key with no text.
    BadLabel,
    /// The template path already taken by a page.
    TakenTemplate,
    /// Columns for a path registered with `back_office`, not
    /// `sheet_upload` (its body limit must not become 0).
    OnBackOffice,
    /// Columns for a path with a parameter.
    Param,
}

struct Grades(Way);

const BAD: &[UploadColumn] = &[UploadColumn::required("x", "grades.col.missing", "")];

async fn page(s: axum::extract::State<AppState>, staff: CurrentStaff) -> Html<String> {
    let body = maud::html! {
        div class="cols" {
            form class="card" { input type="file" name="file"; }
            (harpseal::ui::upload_help(&s, &staff, "/grades/upload"))
        }
    };
    Html(harpseal::ui::back_office(&s, &staff, "grades", body).into_string())
}

impl Module for Grades {
    fn name(&self) -> &'static str {
        "grades"
    }
    fn register(&self, r: &mut ModuleRegistrar) {
        r.permission(Permission::new("grades.import", "Import grades").thai("นำเข้าผลการเรียน"))
            .messages(TEXTS)
            .back_office_page(
                "kernel.student.view",
                "/grades",
                "grades.title",
                60,
                get(page),
            );
        let upload = post(|| async { "kept" });
        match self.0 {
            Way::After => {
                r.sheet_upload("grades.import", "/grades/upload", upload)
                    .sheet_columns("/grades/upload", COLUMNS);
            }
            Way::Before => {
                r.sheet_columns("/grades/upload", COLUMNS).sheet_upload(
                    "grades.import",
                    "/grades/upload",
                    upload,
                );
            }
            Way::None => {
                r.sheet_upload("grades.import", "/grades/upload", upload);
            }
            Way::Orphan => {
                r.sheet_columns("/grades/elsewhere", COLUMNS);
            }
            Way::BadLabel => {
                r.sheet_upload("grades.import", "/grades/upload", upload)
                    .sheet_columns("/grades/upload", BAD);
            }
            Way::OnBackOffice => {
                r.sheet_columns("/grades/upload", COLUMNS).back_office(
                    "grades.import",
                    "/grades/upload",
                    upload,
                );
            }
            Way::Param => {
                r.sheet_upload("grades.import", "/grades/{term}/upload", upload)
                    .sheet_columns("/grades/{term}/upload", COLUMNS);
            }
            Way::TakenTemplate => {
                r.sheet_upload("grades.import", "/grades/upload", upload)
                    .sheet_columns("/grades/upload", COLUMNS)
                    .back_office(
                        "grades.import",
                        "/grades/upload/template.csv",
                        get(|| async { "mine" }),
                    );
            }
        }
    }
}

fn build(h: &Harness, way: Way) -> Result<harpseal::Deployment, BuildError> {
    h.builder()
        .config(DeploymentConfig::from_toml(CONFIG).unwrap())
        .module(Grades(way))
        .build()
}

async fn boot(way: Way) -> (Harness, TestApp) {
    let h = Harness::new();
    let app = h.boot(build(&h, way).unwrap()).await;
    (h, app)
}

async fn as_role<'a>(h: &Harness, app: &'a TestApp, role: &str) -> Browser<'a> {
    let email = format!("{}@sealbay.example", role.to_lowercase());
    h.create_staff(&email, PASSWORD, &[role]).await;
    let b = app.browser();
    b.login(&email, PASSWORD).await;
    b
}

#[tokio::test]
async fn declared_columns_give_the_panel_and_the_template() {
    for way in [Way::After, Way::Before] {
        let (h, app) = boot(way).await;
        let b = as_role(&h, &app, "Lecturer").await;
        let page = b.get("/admin/grades").await.body;
        assert!(page.contains("ไฟล์ CSV หรือ XLSX ขนาดไม่เกิน 4 MB"), "{page}");
        assert!(page.contains("หัวคอลัมน์ต้องสะกดตามรายการด้านล่าง"), "{page}");
        assert!(!page.contains("เรียงลำดับใดก็ได้"), "no promise for a Module");
        for (head, label) in [
            ("รหัสนักศึกษา", "รหัสนักศึกษา 8 หลัก"),
            ("รหัสวิชา", "รหัสวิชา"),
            ("หมายเหตุ", "หมายเหตุถึงนักศึกษา"),
        ] {
            assert!(
                page.contains(&format!("<span class=\"name code-head\">{head}</span>")),
                "{page}"
            );
            assert!(page.contains(label), "{page}");
        }
        assert!(
            page.contains("<span class=\"example\">SB101</span>"),
            "{page}"
        );
        assert!(page.contains("เว้นว่างได้"), "{page}");
        let href = "href=\"/admin/grades/upload/template.csv\" download";
        assert!(page.contains(href), "{page}");
        let t = b.get("/admin/grades/upload/template.csv").await;
        assert_eq!(t.status, 200);
        assert_eq!(t.body, "\u{feff}รหัสนักศึกษา,รหัสวิชา,หมายเหตุ\r\n");
        assert_eq!(t.header("content-type"), Some("text/csv; charset=utf-8"));
        let disposition = t.header("content-disposition").unwrap_or_default();
        assert!(disposition.contains("grades-template.csv"), "{disposition}");
    }
}

#[tokio::test]
async fn the_template_needs_the_uploads_permission() {
    let (h, app) = boot(Way::After).await;
    let b = as_role(&h, &app, "Viewer").await;
    assert_eq!(b.get("/admin/grades/upload/template.csv").await.status, 403);
    // The panel still shows the columns, without the link they cannot use.
    let page = b.get("/admin/grades").await.body;
    assert!(
        page.contains("<span class=\"example\">SB101</span>"),
        "{page}"
    );
    assert!(!page.contains("template.csv"), "{page}");
}

#[tokio::test]
async fn a_module_without_columns_still_gets_the_file_rules() {
    let (h, app) = boot(Way::None).await;
    let b = as_role(&h, &app, "Lecturer").await;
    let page = b.get("/admin/grades").await.body;
    assert!(page.contains("ไฟล์ที่ต้องเตรียม"), "{page}");
    assert!(page.contains("ไฟล์ CSV หรือ XLSX ขนาดไม่เกิน 4 MB"), "{page}");
    assert!(!page.contains("template.csv"), "{page}");
    assert!(!page.contains("upload-columns"), "{page}");
    assert_eq!(b.get("/admin/grades/upload/template.csv").await.status, 404);
}

#[test]
fn build_refuses_columns_it_cannot_serve() {
    let h = Harness::new();
    assert!(
        matches!(build(&h, Way::Orphan), Err(BuildError::InvalidRoute { path, .. }) if path == "/grades/elsewhere")
    );
    assert!(
        matches!(build(&h, Way::BadLabel), Err(BuildError::UnknownText { key, .. }) if key == "grades.col.missing")
    );
    assert!(matches!(
        build(&h, Way::TakenTemplate),
        Err(BuildError::DuplicateRoute { .. })
    ));
    assert!(
        matches!(build(&h, Way::OnBackOffice), Err(BuildError::InvalidRoute { path, .. }) if path == "/grades/upload")
    );
    assert!(
        matches!(build(&h, Way::Param), Err(BuildError::InvalidRoute { path, .. }) if path == "/grades/{term}/upload")
    );
}
