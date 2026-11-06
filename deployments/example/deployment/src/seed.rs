//! `harpseal seed-example`: demo data for the fictional Seal Bay
//! University, written through the same audited paths the back office
//! uses ([`harpseal::roster`], [`harpseal::modules::announcements`],
//! [`harpseal::texts`]) as an existing Administrator, with
//! `RequestCtx::cli().seeded()` so every audit row carries `"seed": true`.
//!
//! It imports the 50-Student roster ([`ROSTER_CSV`], upsert mode, applied
//! at once by running the import Jobs), creates 3 draft announcements and
//! 1 scheduled a week ahead, and overrides 2 texts. Re-running adds
//! nothing twice: the import is an upsert, announcements are matched by
//! their Thai title, and a text that already has any override (the seed's
//! or one an Administrator customised) is left alone. It is refused on a
//! database that already has Students unless `force`, and it never creates
//! Staff or writes a password: create the Administrator first with
//! `harpseal staff create --admin`.
//!
//! **Demo databases only.** While it waits for the roster import, the seed
//! calls `Deployment::tick`, which runs *every* due Job in the CLI's
//! Deployment: one built without the Fake Channel and with the real
//! EmailSender. On a live database pending announcement deliveries would run
//! there (with no Fake Channel) and Quota emails could go out. Never run
//! `seed-example --force` against a production database.

use harpseal::audience::Audience;
use harpseal::modules::announcements::{self, NewAnnouncement};
use harpseal::roster::{self, Confirmed, Uploaded};
use harpseal::{Actor, Deployment, RequestCtx, Statement};

/// The example roster: 50 fictional Students (`data/roster.csv`).
pub const ROSTER_CSV: &[u8] = include_bytes!("../../data/roster.csv");
/// The same 50 Students as an XLSX workbook (`data/roster.xlsx`).
pub const ROSTER_XLSX: &[u8] = include_bytes!("../../data/roster.xlsx");

/// Thai titles of the seeded announcements; the last one is scheduled.
pub const ANNOUNCEMENT_TITLES: [&str; 4] = [
    "ปฐมนิเทศนักศึกษาใหม่ ปีการศึกษา 2569",
    "ปิดปรับปรุงระบบลงทะเบียนเรียน",
    "เปิดรับสมัครทุนการศึกษาหลักสูตรนานาชาติ",
    "ซ้อมใหญ่ชมรมดนตรีประจำภาคเรียน",
];

/// Text overrides the seed writes: (key, Locale, value).
pub const TEXT_OVERRIDES: [(&str, &str, &str); 2] = [
    (
        "kernel.flow.welcome",
        "th",
        "สวัสดีจากมหาวิทยาลัยอ่าวแมวน้ำ (ระบบสาธิต) กรุณาอ่านและยอมรับประกาศความเป็นส่วนตัวด้านล่างก่อนเริ่มใช้งาน",
    ),
    (
        "kernel.flow.welcome",
        "en",
        "Hello from Seal Bay University (demo). Please read and accept the privacy notice below before we start.",
    ),
];

/// Most ticks spent waiting for the roster import to finish.
const MAX_TICKS: usize = 100;

/// What [`seed`] did.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct SeedReport {
    /// The acting Administrator's email.
    pub acting_as: String,
    /// The roster import's id.
    pub import_id: String,
    /// Students in the database afterwards.
    pub students: i64,
    /// Announcements created now (0 on a re-run).
    pub announcements_created: usize,
    /// Text overrides written now (0 on a re-run, and for every text that
    /// already had an override).
    pub texts_written: usize,
}

impl std::fmt::Display for SeedReport {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "seeded as {}: roster import {} ({} Students), {} announcements created, \
             {} texts written",
            self.acting_as,
            self.import_id,
            self.students,
            self.announcements_created,
            self.texts_written
        )
    }
}

/// Why [`seed`] stopped.
#[derive(Debug)]
#[non_exhaustive]
pub enum SeedError {
    /// No active Administrator (or none with the given email).
    NoAdministrator,
    /// The database already has this many Students; pass `force`.
    NotEmpty(i64),
    /// The roster import was refused, busy or failed.
    Import(String),
    /// The Core failed.
    Core(harpseal::Error),
}

impl std::fmt::Display for SeedError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SeedError::NoAdministrator => f.write_str(
                "no active Administrator to act as: run `harpseal staff create --admin` first",
            ),
            SeedError::NotEmpty(n) => write!(
                f,
                "the database already has {n} Students; add --force to seed anyway"
            ),
            SeedError::Import(m) => write!(f, "roster import: {m}"),
            SeedError::Core(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for SeedError {}

impl From<harpseal::Error> for SeedError {
    fn from(e: harpseal::Error) -> Self {
        SeedError::Core(e)
    }
}

async fn query(d: &Deployment, st: Statement) -> Result<harpseal::Rows, SeedError> {
    Ok(d.services().db.query(st).await?)
}

async fn count(d: &Deployment, sql: &'static str) -> Result<i64, SeedError> {
    let rows = query(d, Statement::new(sql)).await?;
    Ok(rows.rows[0][0].as_i64().unwrap_or(0))
}

/// The acting Administrator: `email`, or the oldest active one.
async fn administrator(d: &Deployment, email: Option<&str>) -> Result<(String, String), SeedError> {
    let sql = "SELECT s.id, s.email FROM staff s JOIN staff_role sr ON sr.staff_id = s.id \
               JOIN role r ON r.id = sr.role_id WHERE r.name = ? AND s.status = 'active' \
               AND (? IS NULL OR s.email = lower(trim(?))) ORDER BY s.created_at, s.id LIMIT 1";
    let st = Statement::new(sql)
        .bind(harpseal::ADMINISTRATOR_ROLE)
        .bind(email.map(str::to_string))
        .bind(email.map(str::to_string));
    let rows = query(d, st).await?;
    let text = |c: &str| rows.get(0, c).and_then(|v| v.as_text()).map(str::to_string);
    match (text("id"), text("email")) {
        (Some(id), Some(email)) => Ok((id, email)),
        _ => Err(SeedError::NoAdministrator),
    }
}

/// Seeds the demo data; see the module docs. `email` picks the acting
/// Administrator (default: the oldest active one).
pub async fn seed(
    d: &Deployment,
    email: Option<&str>,
    force: bool,
) -> Result<SeedReport, SeedError> {
    d.migrate().await?;
    let (staff_id, acting_as) = administrator(d, email).await?;
    let students = count(d, "SELECT count(*) FROM student").await?;
    if students > 0 && !force {
        return Err(SeedError::NotEmpty(students));
    }
    let actor = Actor::Staff(staff_id);
    let ctx = RequestCtx::cli().seeded();
    let import_id = import_roster(d, &actor, &ctx).await?;
    let announcements_created = announce(d, &actor, &ctx).await?;
    let mut texts_written = 0;
    for (key, locale, value) in TEXT_OVERRIDES {
        if has_override(d, key, locale).await? {
            continue;
        }
        if harpseal::texts::set_override(d, key, locale, value, &actor, &ctx).await? {
            texts_written += 1;
        }
    }
    Ok(SeedReport {
        acting_as,
        import_id,
        students: count(d, "SELECT count(*) FROM student").await?,
        announcements_created,
        texts_written,
    })
}

/// Whether `key` in `locale` already has a `setting` row (any value).
async fn has_override(d: &Deployment, key: &str, locale: &str) -> Result<bool, SeedError> {
    let st = Statement::new("SELECT count(*) FROM setting WHERE key = ? AND locale = ?")
        .bind(key)
        .bind(locale);
    let rows = query(d, st).await?;
    Ok(rows.rows[0][0].as_i64().unwrap_or(0) > 0)
}

/// Uploads, confirms and applies the roster (upsert); returns the import id.
async fn import_roster(
    d: &Deployment,
    actor: &Actor,
    ctx: &RequestCtx,
) -> Result<String, SeedError> {
    let bytes = ROSTER_CSV.to_vec();
    let id = match roster::upload(d, "roster.csv", bytes, "upsert", actor, ctx).await? {
        Uploaded::Previewed(id) => id,
        Uploaded::Refused(id) => {
            return Err(SeedError::Import(format!(
                "refused, see /admin/roster/{id}"
            )));
        }
        Uploaded::Busy(id) => {
            return Err(SeedError::Import(format!("import {id} is in progress")));
        }
        other => return Err(SeedError::Import(format!("{other:?}"))),
    };
    match roster::confirm(d, &id, actor, ctx).await? {
        Confirmed::Queued => {}
        other => return Err(SeedError::Import(format!("{other:?}"))),
    }
    for _ in 0..MAX_TICKS {
        let state = query(
            d,
            Statement::new("SELECT state, error FROM roster_import WHERE id = ?").bind(id.as_str()),
        )
        .await?;
        match state.get(0, "state").and_then(|v| v.as_text()) {
            Some("done") => return Ok(id),
            Some("failed" | "cancelled") => {
                let why = state.get(0, "error").and_then(|v| v.as_text());
                return Err(SeedError::Import(why.unwrap_or("failed").to_string()));
            }
            _ => {}
        }
        d.tick(d.services().clock.now()).await?;
    }
    Err(SeedError::Import(format!("import {id} did not finish")))
}

fn audience(attr: &str, value: &str) -> Audience {
    let mut a = Audience::everyone();
    a.attributes.insert(attr.into(), vec![value.into()]);
    a
}

/// The four announcements, each only if its Thai title is not there yet.
fn announcements(now: i64) -> Vec<(NewAnnouncement, bool)> {
    let [orientation, maintenance, scholarship, rehearsal] = ANNOUNCEMENT_TITLES;
    let mut a = NewAnnouncement::new(
        orientation,
        "ขอเชิญนักศึกษาชั้นปีที่ 1 เข้าร่วมปฐมนิเทศ ณ หอประชุมอ่าวแมวน้ำ วันจันทร์ เวลา 09:00 น. \
         กรุณากดรับทราบเมื่ออ่านแล้ว (ข้อความสาธิต)",
    );
    a.audience = audience("year", "1");
    a.require_ack = true;
    let mut b = NewAnnouncement::new(
        maintenance,
        "ระบบลงทะเบียนเรียนจะปิดปรับปรุงคืนวันเสาร์ 22:00–02:00 น. ขออภัยในความไม่สะดวก (ข้อความสาธิต)",
    );
    b.en = Some((
        "Registration system maintenance".into(),
        "The registration system is down for maintenance on Saturday night, 22:00–02:00. (demo)"
            .into(),
    ));
    let mut c = NewAnnouncement::new(
        scholarship,
        "นักศึกษาหลักสูตร DDS-Intl ยื่นใบสมัครทุนได้ที่กองกิจการนักศึกษาภายในสิ้นเดือนนี้ (ข้อความสาธิต)",
    );
    c.audience = audience("programme", "DDS-Intl");
    let mut s = NewAnnouncement::new(
        rehearsal,
        "สมาชิกชมรมดนตรีซ้อมใหญ่ที่ห้องดนตรี อาคาร 3 สัปดาห์หน้า เวลา 17:00 น. (ข้อความสาธิต)",
    );
    s.audience.tags = vec!["ชมรมดนตรี".into()];
    s.send_at = Some(now + 7 * 24 * 60 * 60 * 1000);
    vec![(a, false), (b, false), (c, false), (s, true)]
}

async fn announce(d: &Deployment, actor: &Actor, ctx: &RequestCtx) -> Result<usize, SeedError> {
    let mut created = 0;
    for (new, schedule) in announcements(d.services().now_ms()) {
        let exists = query(
            d,
            Statement::new(
                "SELECT count(*) FROM announcement_text WHERE locale = 'th' AND title = ?",
            )
            .bind(new.title_th.as_str()),
        )
        .await?;
        if exists.rows[0][0].as_i64().unwrap_or(0) > 0 {
            continue;
        }
        let id = announcements::create_draft(d, &new, actor, ctx).await?;
        if schedule {
            announcements::confirm(d, &id, 0, actor, ctx).await?;
        }
        created += 1;
    }
    Ok(created)
}
