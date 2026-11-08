//! The steps, in order: Staff sign-in, two roster imports, one Student
//! from follow to Link, an announcement with acknowledgement, the Mini
//! App, the admin pages and the self-unlink. Each returns what it saw;
//! the Student's side is in `chat.rs`.

use std::path::PathBuf;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde_json::Value;

use crate::client::{Client, R, urlencode};

/// The example roster, the same rows in both formats.
const ROSTER_CSV: &[u8] = include_bytes!("../../data/roster.csv");
const ROSTER_XLSX: &[u8] = include_bytes!("../../data/roster.xlsx");

/// The Staff member `just e2e` creates.
pub const ADMIN: &str = "admin@sealbay.example";
const NEW_PASSWORD: &str = "kelp forest at dawn, e2e";
/// The Student the walk plays (first row of the roster, year 1).
pub(crate) const STUDENT: &str = "e2e-student-68010001";
pub(crate) const CODE: &str = "68010001";
pub(crate) const EMAIL: &str = "s68010001@sealbay.example";

/// The walk's state between steps.
pub struct World {
    pub(crate) c: Client,
    db: PathBuf,
    pub(crate) seen: Mutex<usize>,
    pub(crate) announcement: Mutex<String>,
}

pub(crate) fn ensure(ok: bool, why: impl Into<String>) -> R<()> {
    if ok { Ok(()) } else { Err(why.into()) }
}

pub(crate) fn actions(m: &Value) -> Vec<String> {
    m["buttons"]
        .as_array()
        .map(|bs| {
            bs.iter()
                .filter_map(|b| b["action"].as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default()
}

pub(crate) fn texts(ms: &[Value]) -> String {
    let all: Vec<&str> = ms.iter().filter_map(|m| m["text"].as_str()).collect();
    all.join(" | ")
}

impl World {
    /// A walk against `c`, reading assertions from the SQLite file `db`.
    pub fn new(c: Client, db: PathBuf) -> Self {
        Self {
            c,
            db,
            seen: Mutex::new(0),
            announcement: Mutex::new(String::new()),
        }
    }

    /// One integer from `sql` with `params`.
    pub(crate) fn int(&self, sql: &str, params: &[&str]) -> R<i64> {
        self.scalar(sql, params)
    }

    /// The first column of the first row of `sql`, read straight from the
    /// SQLite file (read-only, waiting up to 5 s for the server's writes).
    fn scalar<T: rusqlite::types::FromSql>(&self, sql: &str, params: &[&str]) -> R<T> {
        let flags = rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY;
        let conn =
            rusqlite::Connection::open_with_flags(&self.db, flags).map_err(|e| e.to_string())?;
        conn.busy_timeout(Duration::from_secs(5))
            .map_err(|e| e.to_string())?;
        conn.query_row(sql, rusqlite::params_from_iter(params), |r| r.get(0))
            .map_err(|e| format!("{sql}: {e}"))
    }

    /// Waits for `/notice` to answer 200 (30 s at most).
    pub async fn up(&self, exited: impl Fn() -> bool) -> R<String> {
        let start = Instant::now();
        while start.elapsed() < Duration::from_secs(30) {
            ensure(!exited(), "the server exited; see its log")?;
            if let Ok(r) = self.c.get("/notice").await
                && r.status == 200
                && r.body.contains("มหาวิทยาลัยอ่าวแมวน้ำ")
            {
                return Ok("/notice 200".into());
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        Err("no answer from /notice within 30 s".into())
    }

    /// Signs in with the CLI's temporary password; lands on the change form.
    pub async fn login(&self, temporary: &str) -> R<String> {
        self.c
            .get("/admin/login")
            .await?
            .expect(200, "login form")?;
        let form = format!(
            "email={}&password={}",
            urlencode(ADMIN),
            urlencode(temporary)
        );
        let to = self
            .c
            .post("/admin/login", &form)
            .await?
            .redirect("sign-in")?;
        let page = self.c.get(&to).await?;
        ensure(
            page.status == 200 || page.status == 303,
            format!("{to}: {}", page.status),
        )?;
        Ok(format!("303 to {to}"))
    }

    /// Changes the temporary password; the back office opens.
    pub async fn change_password(&self, temporary: &str) -> R<String> {
        self.c
            .get("/admin/password")
            .await?
            .expect(200, "change form")?;
        let new = urlencode(NEW_PASSWORD);
        let form = format!("current={}&new={new}&confirm={new}", urlencode(temporary));
        self.c
            .post("/admin/password", &form)
            .await?
            .redirect("change")?;
        self.c.get("/admin").await?.expect(200, "/admin")?;
        Ok("/admin 200".into())
    }

    /// Uploads, previews, confirms and ticks until the import is done.
    async fn import(&self, name: &str, bytes: &[u8], mode: &str) -> R<String> {
        self.c
            .get("/admin/roster")
            .await?
            .expect(200, "roster page")?;
        let to = self
            .c
            .post_file("/admin/roster/upload", &[("mode", mode)], name, bytes)
            .await?
            .redirect("upload")?;
        let id = to
            .strip_prefix("/admin/roster/")
            .ok_or(format!("upload went to {to}"))?
            .to_string();
        self.c.get(&to).await?.expect(200, "preview")?;
        let state = |w: &Self| -> R<String> {
            w.scalar("SELECT state FROM roster_import WHERE id = ?", &[&id])
        };
        ensure(
            state(self)? == "previewed",
            format!("import {id} is {}", state(self)?),
        )?;
        self.c
            .post(&format!("{to}/confirm"), "")
            .await?
            .redirect("confirm")?;
        for _ in 0..60 {
            match state(self)?.as_str() {
                "done" => return Ok(id),
                "failed" | "cancelled" => return Err(format!("import {id} {}", state(self)?)),
                _ => {}
            }
            self.c.tick().await?;
            self.c.get(&to).await?.expect(200, "progress")?;
        }
        Err(format!("import {id} never finished"))
    }

    fn students(&self) -> R<(i64, i64, i64)> {
        Ok((
            self.int("SELECT count(*) FROM student", &[])?,
            self.int("SELECT count(*) FROM student WHERE status = 'active'", &[])?,
            self.int("SELECT count(*) FROM student_tag", &[])?,
        ))
    }

    /// The 50-row CSV in `full` mode.
    pub async fn roster_csv(&self) -> R<String> {
        let id = self.import("roster.csv", ROSTER_CSV, "full").await?;
        let (all, active, tags) = self.students()?;
        ensure(
            (all, active) == (50, 50),
            format!("{all} Students, {active} active"),
        )?;
        Ok(format!("import {id}: 50 Students, {tags} Tags"))
    }

    /// The same rows as XLSX in `upsert` mode: nothing changes.
    pub async fn roster_xlsx(&self) -> R<String> {
        let before = self.students()?;
        let id = self.import("roster.xlsx", ROSTER_XLSX, "upsert").await?;
        let after = self.students()?;
        ensure(
            before == after,
            format!("counts {before:?} became {after:?}"),
        )?;
        Ok(format!("import {id}: unchanged {after:?}"))
    }

    /// Staff send an announcement needing acknowledgement to year 1: the
    /// group chosen explicitly, then the review step (#120) confirms it.
    pub async fn announce(&self) -> R<String> {
        self.c
            .get("/admin/announcements/new")
            .await?
            .expect(200, "create form")?;
        let form = format!(
            "title_th={}&body_th={}&require_ack=1&scope=groups&attr.year=1&when=now&do=confirm",
            urlencode("ปฐมนิเทศ (e2e)"),
            urlencode("ทดสอบส่งประกาศจาก just e2e กรุณากดรับทราบ")
        );
        let review = self
            .c
            .post("/admin/announcements/new", &form)
            .await?
            .redirect("create")?;
        let id = review
            .strip_prefix("/admin/announcements/")
            .and_then(|r| r.strip_suffix("/review"))
            .ok_or_else(|| format!("not the review step: {review}"))?
            .to_string();
        let page = self.c.get(&review).await?.expect(200, "review")?;
        ensure(page.body.contains("ตรวจก่อนส่ง"), "the review step")?;
        let to = self
            .c
            .post(
                &format!("/admin/announcements/{id}/confirm"),
                "version=0&reviewed=1",
            )
            .await?
            .redirect("confirm")?;
        self.c.get(&to).await?.expect(200, "detail")?;
        *self.announcement.lock().unwrap_or_else(|p| p.into_inner()) = id.clone();
        Ok(format!("announcement {id}"))
    }

    /// Usage, Security Events and the audit log answer 200.
    pub async fn admin_pages(&self) -> R<String> {
        for p in ["/admin/usage", "/admin/security", "/admin/audit"] {
            self.c.get(p).await?.expect(200, p)?;
        }
        Ok("usage, security, audit: 200".into())
    }

    /// Branding (#114): a too-light colour is refused in Thai; a new name
    /// and colour are saved, audited and shown in the back office and the
    /// Mini App, then returned to the TOML.
    pub async fn branding(&self) -> R<String> {
        const NAME: &str = "มหาวิทยาลัยหาดแมวน้ำ (e2e)";
        // The version of the values the form shows: a stale form is refused.
        let seen = |body: &str| -> R<String> {
            let mark = "name=\"seen\" value=\"";
            let at = body.find(mark).ok_or("no seen field")? + mark.len();
            let end = body[at..].find('"').ok_or("no seen end")?;
            Ok(body[at..at + end].to_string())
        };
        let page = self.c.get("/admin/branding").await?.expect(200, "page")?;
        let v = seen(&page.body)?;
        let save = [
            ("seen", v.as_str()),
            ("op", "save"),
            ("name", NAME),
            ("color", "#ffd400"),
        ];
        let r = self
            .c
            .post_file("/admin/branding/save", &save, "", b"")
            .await?;
        let r = r.expect(422, "light colour")?;
        ensure(r.body.contains("อ่านยาก"), "refusal in Thai")?;
        let save = [
            ("seen", v.as_str()),
            ("op", "save"),
            ("name", NAME),
            ("color", "#1b4f72"),
        ];
        let r = self
            .c
            .post_file("/admin/branding/save", &save, "", b"")
            .await?;
        r.expect(303, "save")?;
        let home = self.c.get("/admin").await?.expect(200, "home")?;
        ensure(home.body.contains("--brand:#1b4f72"), "back office colour")?;
        let notice = self.c.get("/notice").await?.expect(200, "notice")?;
        ensure(notice.body.contains(NAME), "Mini App name")?;
        let n = self.int(
            "SELECT count(*) FROM audit_log WHERE action = 'branding.update'",
            &[],
        )?;
        ensure(n == 1, format!("{n} audit rows"))?;
        let page = self.c.get("/admin/branding").await?.expect(200, "page")?;
        let v = seen(&page.body)?;
        let reset = [("seen", v.as_str()), ("op", "reset")];
        let r = self
            .c
            .post_file("/admin/branding/save", &reset, "", b"")
            .await?;
        r.expect(303, "reset")?;
        let n = self.int(
            "SELECT count(*) FROM setting WHERE key LIKE 'kernel.branding.%'",
            &[],
        )?;
        ensure(n == 0, format!("{n} branding settings left"))?;
        Ok("refused, saved, shown, reset".into())
    }
}
