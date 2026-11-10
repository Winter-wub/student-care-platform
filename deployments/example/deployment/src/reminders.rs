//! A customer Module that reminds chosen Students (#73): every Monday at
//! 09:00 the Students tagged `fee_due` get a fee reminder, by chat when
//! linked, by email otherwise ([`harpseal::JobContext::notify`]), the
//! email linking to the fees page in LINE (#163, `reminders/notice.rs`)
//! while the switch [`EMAIL_LINK`] is on; Staff
//! can also send it now to a pasted list from `/admin/reminders`
//! ([`harpseal::PageState::notify`]), through a review step that shows
//! who gets it, by which channel, and the message (`reminders/review.rs`,
//! #135); the page also shows the weekly schedule. The same form drafts an
//! Announcement to the pasted Students ([`ANNOUNCE_PATH`], #106) through
//! `harpseal::modules::announcements::create_draft`, as the signed-in
//! Staff member, who must also hold `announcements.send`. The list is the
//! Core's Student picker (#146, `reminders/form.rs`): search by code or
//! name, paste many, unknown codes flagged, a textarea without JS; and
//! "เริ่มจากรายการค้างชำระ" fills it with the Students who owe a fee.
//!
//! The weekly Job picks the Students and enqueues one `reminders.send` Job
//! per [`MAX_NOTIFY`] of them, so each run stays within the Workers
//! subrequest budget. The notice key names the week: a retried run, or the
//! Job enqueued twice, sends nobody the reminder twice.

// axum at the Core's version (#162): this crate has no axum dependency.
use harpseal::web::axum;

use axum::Form;
use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::response::{Html, IntoResponse, Redirect, Response};
use axum::routing::{get, post};
use harpseal::audience::{Audience, MAX_STUDENTS, parse_student_ids};
use harpseal::modules::announcements::{NewAnnouncement, create_draft};
use harpseal::{
    Actor, AppState, CurrentStaff, Error, Job, JobContext, JobSpec, MAX_NOTIFY, Module,
    ModuleRegistrar, NotifyDefer, NotifyOutcome, NotifyTo, Permission, RequestCtx, Result,
    SettingKind, Statement, Timestamp, Value, ui,
};
use maud::html;

/// The weekly Job.
pub const WEEKLY: &str = "reminders.weekly";
/// When the weekly Job runs: Mondays at [`HOUR`]:00, local time.
pub const SCHEDULE: &str = "0 9 * * 1";
/// The hour of [`SCHEDULE`], for the home's fees card (#123).
pub const HOUR: i64 = 9;
/// One chunk of the weekly reminder.
pub const SEND: &str = "reminders.send";
/// The Tag that selects the Students.
pub const TAG: &str = "fee_due";
/// The back-office page.
pub const PATH: &str = "/reminders";
/// The page action that drafts an Announcement to the pasted Students.
pub const ANNOUNCE_PATH: &str = "/reminders/announce";
/// The Student picker's search route (#146), under [`PERMISSION`].
pub const PICK: ui::PickerRoute = ui::PickerRoute::new("/reminders/pick");
/// Who may send the reminder from the page.
pub const PERMISSION: Permission =
    Permission::new("reminders.send", "Send the fee reminder").thai("ส่งเตือนชำระค่าธรรมเนียม");

const DAY_MS: i64 = 86_400_000;

mod form;
mod notice;
mod review;
mod schedule;
mod texts;
pub use notice::{EMAIL_LINK, fees_url, notice};
use notice::{FEES_PAGE, NOTICE_TH};
use texts::TEXTS;

/// The Module.
pub struct Reminders;

impl Module for Reminders {
    fn name(&self) -> &'static str {
        "reminders"
    }

    fn register(&self, r: &mut ModuleRegistrar) {
        r.permission(PERMISSION)
            .messages(TEXTS)
            .messages(notice::TEXTS)
            .setting(
                EMAIL_LINK,
                SettingKind::Boolean,
                "1",
                PERMISSION.id,
                "reminders.email_link_label",
                "reminders.email_link_help",
            )
            .job(JobSpec::scheduled(WEEKLY, SCHEDULE), weekly)
            .job(JobSpec::new(SEND), send)
            .back_office_page(
                PERMISSION.id,
                PATH,
                "reminders.title",
                60,
                get(page).post(send_now),
            )
            .back_office(PERMISSION.id, ANNOUNCE_PATH, post(announce))
            .student_picker(PERMISSION.id, PICK);
    }
}

/// `reminders.weekly`: the tagged Students, one `reminders.send` per chunk.
async fn weekly(cx: JobContext, _job: Job) -> Result<()> {
    let offset = i64::from(cx.config.utc_offset_minutes()?) * 60_000;
    // Weeks counted from the epoch, which was a Thursday: each runs
    // Thursday to Wednesday (local), so every Monday run has its own week.
    let week = (cx.services.now_ms() + offset) / DAY_MS / 7;
    // The Kernel scopes the key by the Module (`reminders:`).
    let key = format!("{TAG}:w{week}");
    let rows = cx
        .services
        .db
        .query(
            Statement::new(
                "SELECT DISTINCT s.student_code FROM student s JOIN student_tag st \
                 ON st.student_id = s.id JOIN tag t ON t.id = st.tag_id \
                 WHERE t.name = ? AND s.status = 'active' ORDER BY s.student_code",
            )
            .bind(TAG),
        )
        .await?;
    let codes: Vec<String> = (0..rows.len())
        .filter_map(|i| rows.get(i, "student_code").and_then(Value::as_text))
        .map(str::to_string)
        .collect();
    let mut stmts = Vec::new();
    for (n, chunk) in codes.chunks(MAX_NOTIFY).enumerate() {
        let payload = serde_json::json!({ "key": key, "codes": chunk });
        let (_, stmt) = cx
            .jobs
            .insert_unique(&format!("{key}:{n}"), SEND, &payload, None)?;
        stmts.push(stmt);
    }
    if !stmts.is_empty() {
        cx.services.db.batch(stmts).await?;
    }
    Ok(())
}

/// `reminders.send`: one chunk. Students over the call's request budget
/// go to a follow-up Job at once, and Students waiting for a platform
/// quota to reset (LINE's monthly limit, no email address) to one a day
/// later; failed ones, or ones another run is still sending to, make this
/// run fail, so the Job's retry (1, 5, then 30 minutes later) tries them
/// again. The key keeps everyone else from a repeat.
async fn send(cx: JobContext, job: Job) -> Result<()> {
    let p: serde_json::Value = serde_json::from_str(&job.payload)
        .map_err(|_| Error::Invalid("job payload is not JSON".into()))?;
    let key = p["key"].as_str().unwrap_or_default();
    let to: Vec<NotifyTo> = p["codes"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|c| c.as_str())
                .map(NotifyTo::code)
                .collect()
        })
        .unwrap_or_default();
    let url = fees_url(&cx.settings(), cx.mini_app_url(FEES_PAGE.0, FEES_PAGE.1)).await?;
    let r = cx.notify(&notice(key, url.as_deref()), &to).await?;
    let note = format!(
        "chat {}, email {}, skipped {}, deferred {}, failed {}",
        r.sent, r.emailed, r.skipped, r.deferred, r.failed
    );
    cx.progress(&note).await?;
    // Over the budget: a follow-up Job now. Waiting for a platform quota
    // to reset (no email address): one tomorrow, until it has.
    let later = Timestamp(cx.services.now_ms() + DAY_MS);
    let waits = [
        (NotifyDefer::Budget, "rest", None),
        (NotifyDefer::Quota, "quota", Some(later)),
    ];
    let mut stmts = Vec::new();
    for (why, suffix, at) in waits {
        let codes: Vec<&str> = r
            .students
            .iter()
            .filter(|n| n.outcome == NotifyOutcome::Deferred(why))
            .map(|n| code(&n.to))
            .collect();
        if codes.is_empty() {
            continue;
        }
        let payload = serde_json::json!({ "key": key, "codes": codes });
        let unique = format!("{}:{suffix}", job.id);
        stmts.push(cx.jobs.insert_unique(&unique, SEND, &payload, at)?.1);
    }
    if !stmts.is_empty() {
        cx.services.db.batch(stmts).await?;
    }
    let busy = NotifyOutcome::Deferred(NotifyDefer::InProgress);
    let retry = r.failed > 0 || r.students.iter().any(|n| n.outcome == busy);
    if retry {
        return Err(Error::Platform(note));
    }
    Ok(())
}

/// The page: the weekly schedule (#135), then the form; its `key` makes
/// a second post of the same form (a double click, a reload) send nobody
/// the reminder twice, even while the first is still sending. `?ids=`
/// fills the list again ("กลับไปแก้ไข" on the review step), at most
/// [`MAX_STUDENTS`] codes and only from a query within
/// [`ui::MAX_PREFILL_QUERY`]; a longer one leaves the list empty.
/// `?from=fees` fills it with the Students who owe a fee (#146).
async fn page(
    State(s): State<AppState>,
    staff: CurrentStaff,
    Query(q): Query<Vec<(String, String)>>,
) -> Response {
    let Ok(key) = s.services.new_id() else {
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    };
    let ids = q
        .iter()
        .find(|(k, v)| k == "ids" && v.len() <= ui::MAX_PREFILL_QUERY)
        .map(|(_, v)| {
            let mut ids = parse_student_ids(v);
            ids.truncate(MAX_STUDENTS);
            ids.join("\n")
        })
        .unwrap_or_default();
    let from_fees = q.iter().any(|(k, v)| k == "from" && v == "fees");
    let (ids, note) = if from_fees {
        match form::owing(&s).await {
            Ok((codes, total)) => {
                let note = form::from_fees_note(&s, &staff, codes.len(), total);
                (codes, Some(note))
            }
            Err(e) => return ui::service_error(&s, &staff, &e),
        }
    } else {
        (parse_student_ids(&ids), None)
    };
    let list = match form::list_form(&s, &staff, &key, ids).await {
        Ok(f) => f,
        Err(e) => return ui::service_error(&s, &staff, &e),
    };
    let t = |k: &str| s.catalog.t(&staff.locale, k);
    let schedule = schedule::card(&s, &staff).await;
    let body = html! {
        // The page header sits above its card, as on every page (#131).
        h1 { (t("reminders.title")) }
        (schedule)
        section class="card" {
            h2 { (t("reminders.now_title")) }
            p class="muted" { (t("reminders.now_help")) }
            (form::from_fees_link(&s, &staff))
            @if let Some(n) = note { (n) }
            (list)
        }
    };
    Html(ui::back_office(&s, &staff, &t("reminders.title"), body).into_string()).into_response()
}

/// "ตรวจก่อนส่งเตือน": the review step (#135) until it is confirmed with the
/// count it showed; then sends the reminder to the pasted student IDs and
/// lists the outcome.
async fn send_now(
    State(s): State<AppState>,
    staff: CurrentStaff,
    Form(form): Form<Vec<(String, String)>>,
) -> Response {
    let field = |k: &str| {
        form.iter()
            .find(|(n, _)| n == k)
            .map_or("", |(_, v)| v.as_str())
    };
    // The form's own id (a UUID): never empty, so no two forms share one.
    let key = field("key");
    let fits = |c: char| c.is_ascii_hexdigit() || c == '-';
    if key.len() != 36 || !key.chars().all(fits) {
        return StatusCode::BAD_REQUEST.into_response();
    }
    let plan = match review::plan(&s, parse_student_ids(field("ids"))).await {
        Ok(p) => p,
        Err(e) => return ui::service_error(&s, &staff, &e),
    };
    let reviewed = field(review::REVIEWED) == "1";
    let seen = field(review::REACH).trim().parse::<usize>().ok();
    if !reviewed || seen != Some(plan.reach()) || plan.blocked().is_some() {
        let changed = reviewed && seen != Some(plan.reach());
        return review::page(&s, &staff, key, &plan, changed).await;
    }
    let to: Vec<NotifyTo> = plan.ids.iter().map(NotifyTo::code).collect();
    let title = s.catalog.t(&staff.locale, "reminders.title");
    let t = |k: &str| s.catalog.t(&staff.locale, k);
    let url = match fees_url(&s.settings(), s.mini_app_url(FEES_PAGE.0, FEES_PAGE.1)).await {
        Ok(url) => url,
        Err(e) => return ui::service_error(&s, &staff, &e),
    };
    let reminder = notice(&format!("page:{key}"), url.as_deref());
    let body = match s.notify(&reminder, &to).await {
        Ok(r) => html! {
            h1 { (title) }
            section class="card" {
                p { (t("reminders.result")
                    .replace("{sent}", &r.sent.to_string())
                    .replace("{emailed}", &r.emailed.to_string())
                    .replace("{skipped}", &r.skipped.to_string())
                    .replace("{deferred}", &r.deferred.to_string())
                    .replace("{failed}", &r.failed.to_string())) }
                ul class="list" {
                    @for n in &r.students {
                        li { span class="code" { (code(&n.to)) } ": " (t(outcome(n.outcome))) }
                    }
                }
            }
        },
        Err(e) => html! { h1 { (title) } section class="card" { p { (e.to_string()) } } },
    };
    Html(ui::back_office(&s, &staff, &title, body).into_string()).into_response()
}

/// Drafts an Announcement (the reminder's text, acknowledgement asked) to
/// the pasted student IDs, as the signed-in Staff member, and opens it on
/// the Core's detail page, where Staff check the Audience and send it.
/// Staff without `announcements.send` get the Core's 403 page.
async fn announce(
    State(s): State<AppState>,
    staff: CurrentStaff,
    ctx: RequestCtx,
    Form(form): Form<Vec<(String, String)>>,
) -> Response {
    let ids = form
        .iter()
        .find(|(n, _)| n == "ids")
        .map(|(_, v)| parse_student_ids(v))
        .unwrap_or_default();
    // An empty list would be no filter at all: everyone. Over
    // MAX_STUDENTS the Core refuses the Audience.
    let refused = match ids.len() {
        0 => Some("reminders.no_ids"),
        n if n > MAX_STUDENTS => Some("reminders.too_many_ids"),
        _ => None,
    };
    if let Some(key) = refused {
        let title = s.catalog.t(&staff.locale, "reminders.title");
        let text = s.catalog.t(&staff.locale, key);
        let body = html! { h1 { (title) } section class="card" { p role="alert" { (text) } } };
        let page = ui::back_office(&s, &staff, &title, body);
        return (StatusCode::UNPROCESSABLE_ENTITY, Html(page.into_string())).into_response();
    }
    let mut audience = Audience::everyone();
    audience.students = ids;
    let mut new = NewAnnouncement::new(
        "เตือนชำระค่าธรรมเนียม",
        "มหาวิทยาลัยซีลเบย์: กรุณาชำระค่าธรรมเนียมการศึกษาภายในวันศุกร์นี้",
    );
    new.en = Some((
        "Fee reminder".into(),
        "Seal Bay University: please pay your tuition fee by this Friday.".into(),
    ));
    new.audience = audience;
    new.require_ack = true;
    match create_draft(s.deployment(), &new, &Actor::staff(&staff), &ctx).await {
        Ok(id) => Redirect::to(&format!("/admin/announcements/{id}")).into_response(),
        Err(e) => ui::service_error(&s, &staff, &e),
    }
}

fn code(to: &NotifyTo) -> &str {
    match to {
        NotifyTo::Code(c) => c,
        _ => "?",
    }
}

/// The catalog key of an outcome.
fn outcome(o: NotifyOutcome) -> &'static str {
    match o {
        NotifyOutcome::Sent => "reminders.outcome.sent",
        NotifyOutcome::Emailed => "reminders.outcome.emailed",
        NotifyOutcome::Skipped(_) => "reminders.outcome.skipped",
        NotifyOutcome::Deferred(_) => "reminders.outcome.deferred",
        NotifyOutcome::Overtaken => "reminders.outcome.overtaken",
        NotifyOutcome::Failed(_) => "reminders.outcome.failed",
        // An outcome the Core adds later: say so, never "failed".
        _ => "reminders.outcome.unknown",
    }
}
