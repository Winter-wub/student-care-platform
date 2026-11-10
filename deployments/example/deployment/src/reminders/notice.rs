//! The reminder itself, in Thai and English, and what its email adds
//! (#163). The email goes only to Students the chat does not reach: no
//! live Link, or every live Link unreachable (they blocked the account).
//!
//! - A Student with a live LINE Link gets the URL of the fees page in
//!   LINE ([`harpseal::JobContext::mini_app_url`]): once they unblock the
//!   account it opens their own page. Behind the switch [`EMAIL_LINK`] (a
//!   Boolean Module setting, on by default).
//! - A Student with a live Link on another Channel only (Telegram,
//!   blocked) gets the plain text: no LINE URL, and no "link first" (they
//!   have).
//! - A Student with no live Link gets no URL, which would open "not
//!   linked" in a chat they never joined, but how to add the Official
//!   Account and link first.
//!
//! [`sends`] splits the Students into those groups, one Notice each,
//! under the one key: the Kernel records each Student's send under it,
//! so a retry sends nobody the reminder twice. The email body depends on
//! the group, so a Student whose group changes between an email the
//! provider accepted but did not confirm (a timeout) and its retry gets
//! the retry refused by Resend (409, the key's body differs: reported
//! `Failed`), never a second email.

use harpseal::{
    AppState, CatalogEntry, DEFAULT_BUDGET, Error, Locale, Message, Notice, NotifyReport, NotifyTo,
    Result, Services, Settings, Statement, Value,
};

/// The switch: whether the reminder email links to the fees page.
pub const EMAIL_LINK: &str = "reminders.email_link";

/// The fees page the email links to, opened in LINE: a Telegram Web App
/// URL opens nothing outside Telegram.
pub(super) const FEES_PAGE: (&str, &str) = ("line", crate::fees::student::PATH);

/// The Thai reminder: the email subject and the text.
pub(super) const NOTICE_TH: (&str, &str) = (
    "เตือนชำระค่าธรรมเนียม",
    "มหาวิทยาลัยซีลเบย์: กรุณาชำระค่าธรรมเนียมการศึกษาภายในวันศุกร์นี้",
);

/// What the email says for a Student with no LINE Link.
const LINK_FIRST: (&str, &str) = (
    "ยังไม่ได้ผูกบัญชีนักศึกษากับแชทของมหาวิทยาลัย: เพิ่มเพื่อนบัญชี LINE ทางการของ\
     มหาวิทยาลัยซีลเบย์ แล้วผูกบัญชีด้วยรหัสนักศึกษาในแชท จากนั้นดูยอดที่ต้องชำระได้ในแชท",
    "Your student account is not linked to the university chat yet: add Seal Bay \
     University's LINE Official Account, link your account in the chat with your \
     student ID, then see what you owe there.",
);

/// The switch's label and help on `/admin/settings`.
pub(super) const TEXTS: &[CatalogEntry] = &[
    CatalogEntry::new(
        "reminders.email_link_label",
        "th",
        "ใส่ลิงก์หน้าค่าธรรมเนียมในอีเมลเตือน",
    ),
    CatalogEntry::new(
        "reminders.email_link_label",
        "en",
        "Link the fees page in the reminder email",
    ),
    CatalogEntry::new(
        "reminders.email_link_help",
        "th",
        "เปิดไว้ นักศึกษาที่ผูกบัญชี LINE ไว้แต่บล็อกแชทจะได้อีเมลพร้อมลิงก์เปิดดูยอดที่ต้องชำระใน LINE",
    ),
    CatalogEntry::new(
        "reminders.email_link_help",
        "en",
        "When on, Students with a LINE Link who blocked the chat get the link to their fees page in LINE by email.",
    ),
];

/// What the reminder email adds after the text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Extra<'a> {
    /// Nothing (the switch is off, or no LIFF app).
    Nothing,
    /// The fees page in LINE: for a Student with a live LINE Link.
    FeesUrl(&'a str),
    /// How to add the Official Account and link first: no LINE Link.
    LinkFirst,
}

/// The reminder, in Thai and English, under `key`, its email ending with
/// `extra`. A chat message needs no URL.
pub fn notice(key: &str, extra: Extra<'_>) -> Notice {
    let (subject, th) = NOTICE_TH;
    let en = "Seal Bay University: please pay your tuition fee by this Friday.";
    let mail = |text: &str, see: &str, first: &str| match extra {
        Extra::Nothing => text.to_string(),
        Extra::FeesUrl(url) => format!("{text}\n\n{see}: {url}"),
        Extra::LinkFirst => format!("{text}\n\n{first}"),
    };
    let th_mail = mail(th, "ดูยอดที่ต้องชำระใน LINE", LINK_FIRST.0);
    let en_mail = mail(en, "See what you owe in LINE", LINK_FIRST.1);
    Notice::new("reminders", key)
        .locale(Locale::th(), vec![Message::text(th)], subject, th_mail)
        .locale(
            Locale::en(),
            vec![Message::text(en)],
            "Fee reminder",
            en_mail,
        )
}

/// `url` (from [`harpseal::JobContext::mini_app_url`] or
/// [`harpseal::PageState::mini_app_url`] for the fees page) while the
/// switch [`EMAIL_LINK`] is on, else `None`. A saved switch that no
/// longer passes its check (`Error::SettingNotSet`; `/admin/settings`
/// flags it) counts as off rather than stopping the reminder; any other
/// error (the database) is returned.
pub async fn fees_url(settings: &Settings, url: Option<String>) -> Result<Option<String>> {
    match settings.bool(EMAIL_LINK).await {
        Ok(true) => Ok(url),
        Ok(false) | Err(Error::SettingNotSet(_)) => Ok(None),
        Err(e) => Err(e),
    }
}

/// The reminder's sends under `key`, one per non-empty group of `to`
/// (see the module docs): a live LINE Link (`url` in the email, when
/// given), a live Link elsewhere only (the plain text), none (how to link
/// first). A Student `to` names by `NotifyTo::Id` or by code is grouped
/// alike; one not found has no Link (the Kernel skips them anyway).
///
/// Budget: each group gets its share of [`DEFAULT_BUDGET`] by size,
/// rounded down but at least 1 (a lone group gets all of it), so the
/// calls together spend about one call's budget: over it by at most one
/// Student's cost per group, as the first Student of each call always
/// goes. Merge the reports with [`merge`].
pub(super) async fn sends(
    services: &Services,
    url: Option<&str>,
    key: &str,
    to: &[NotifyTo],
) -> Result<Vec<(Notice, Vec<NotifyTo>)>> {
    let linked = live_links(services, to).await?;
    let mut groups: [(Extra<'_>, Vec<NotifyTo>); 3] = [
        (url.map_or(Extra::Nothing, Extra::FeesUrl), Vec::new()),
        (Extra::Nothing, Vec::new()),
        (Extra::LinkFirst, Vec::new()),
    ];
    for t in to {
        let channels: Vec<&str> = linked
            .iter()
            .filter(|(id, code, _)| match t {
                NotifyTo::Code(c) => c == code,
                NotifyTo::Id(i) => i.0 == *id,
                _ => false,
            })
            .map(|(_, _, ch)| ch.as_str())
            .collect();
        let g = match () {
            _ if channels.contains(&"line") => 0,
            _ if !channels.is_empty() => 1,
            _ => 2,
        };
        groups[g].1.push(t.clone());
    }
    let lone = groups.iter().filter(|(_, g)| !g.is_empty()).count() == 1;
    let share = |n: usize| match lone {
        true => DEFAULT_BUDGET,
        false => (DEFAULT_BUDGET * n / to.len().max(1)).max(1),
    };
    Ok(groups
        .into_iter()
        .filter(|(_, g)| !g.is_empty())
        .map(|(x, g)| (notice(key, x).budget(share(g.len())), g))
        .collect())
}

/// (student id, student code, Channel) of every live (active or
/// unreachable) Link of the Students of `to`.
async fn live_links(services: &Services, to: &[NotifyTo]) -> Result<Vec<(String, String, String)>> {
    let keys: Vec<&str> = to
        .iter()
        .filter_map(|t| match t {
            NotifyTo::Code(c) => Some(c.as_str()),
            NotifyTo::Id(i) => Some(i.0.as_str()),
            _ => None,
        })
        .collect();
    if keys.is_empty() {
        return Ok(Vec::new());
    }
    let marks = vec!["?"; keys.len()].join(", ");
    let mut q = Statement::new(format!(
        "SELECT DISTINCT s.id, s.student_code, l.channel FROM student s \
         JOIN link l ON l.student_id = s.id \
         WHERE l.status IN ('active', 'unreachable') \
         AND (s.student_code IN ({marks}) OR s.id IN ({marks}))"
    ));
    for k in keys.iter().chain(&keys) {
        q = q.bind(*k);
    }
    let rows = services.db.query(q).await?;
    let text = |i: usize, c: &str| rows.get(i, c).and_then(Value::as_text).unwrap_or_default();
    Ok((0..rows.len())
        .map(|i| (text(i, "id"), text(i, "student_code"), text(i, "channel")))
        .map(|(a, b, c)| (a.to_string(), b.to_string(), c.to_string()))
        .collect())
}

/// One report from the reports of [`sends`], its Students back in the
/// order of `to`.
pub(super) fn merge(to: &[NotifyTo], reports: Vec<NotifyReport>) -> NotifyReport {
    let mut all = NotifyReport::default();
    for r in reports {
        all.sent += r.sent;
        all.emailed += r.emailed;
        all.skipped += r.skipped;
        all.deferred += r.deferred;
        all.failed += r.failed;
        all.overtaken += r.overtaken;
        all.students.extend(r.students);
        all.exhausted.extend(r.exhausted);
    }
    // Sorted and once each, as the Core reports them.
    all.exhausted.sort();
    all.exhausted.dedup();
    let at = |n: &NotifyTo| to.iter().position(|t| t == n).unwrap_or(usize::MAX);
    all.students.sort_by_key(|n| at(&n.to));
    all
}

/// The reminder to `to` from the page, every group ([`sends`]): the
/// merged report of the groups sent, and the error that stopped the rest,
/// if one did. What went out before it is recorded under the key and
/// shown, so a Staff member sees who got it; sending the same form again
/// reports those as done and sends only the rest. (The weekly Job instead
/// fails and is retried, which the key makes safe.)
pub(super) async fn remind(
    s: &AppState,
    url: Option<&str>,
    key: &str,
    to: &[NotifyTo],
) -> (NotifyReport, Option<Error>) {
    let groups = match sends(&s.services, url, key, to).await {
        Ok(g) => g,
        Err(e) => return (NotifyReport::default(), Some(e)),
    };
    let mut reports = Vec::new();
    for (n, group) in groups {
        match s.notify(&n, &group).await {
            Ok(r) => reports.push(r),
            Err(e) => return (merge(to, reports), Some(e)),
        }
    }
    (merge(to, reports), None)
}
