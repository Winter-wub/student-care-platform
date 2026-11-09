//! The back office in plain Thai (#125), on the seeded example: no raw
//! catalog key on a main page outside the "ข้อมูลทางเทคนิค" disclosure,
//! Thai dates, Thai Audience and Role labels, and Role labels that leave
//! the stored names alone.

use example_deployment::{Secrets, builder, seed::seed};
use harpseal_testing::{Browser, Harness, TestApp};

const PASSWORD: &str = "kelp forest at dawn";

fn secrets() -> Secrets {
    Secrets::from_lookup(|k| match k {
        "LINE_LIFF_ID" => Some("1234567890-AbCdEfGh".into()),
        "TELEGRAM_WEB_APP_URL" => Some("https://app.seal-bay.invalid/tg".into()),
        _ => None,
    })
}

async fn seeded(h: &Harness) -> TestApp {
    let d = builder(h.services(), secrets())
        .unwrap()
        .channel(h.fake_channel())
        .build()
        .expect("the example Deployment builds");
    let app = h.boot(d).await;
    h.create_staff("admin@sealbay.example", PASSWORD, &["Administrator"])
        .await;
    h.create_staff("officer@sealbay.example", PASSWORD, &["Officer"])
        .await;
    seed(app.deployment(), Some("admin@sealbay.example"), false)
        .await
        .expect("seeds");
    app
}

/// The text a reader sees: no `<details class="tech">` (nested details
/// included), scripts, styles, tags or attribute values.
fn visible(html: &str) -> String {
    let mut out = String::new();
    let mut rest = html;
    while let Some(at) = rest.find("<details class=\"tech\">") {
        out.push_str(&rest[..at]);
        let mut depth = 0usize;
        let mut i = at;
        loop {
            let open = rest[i..].find("<details").map(|o| i + o);
            let close = rest[i..].find("</details>").map(|c| i + c);
            match (open, close) {
                (Some(o), Some(c)) if o < c => {
                    depth += 1;
                    i = o + 8;
                }
                (_, Some(c)) => {
                    depth -= 1;
                    i = c + 10;
                    if depth == 0 {
                        break;
                    }
                }
                _ => panic!("unclosed disclosure"),
            }
        }
        rest = &rest[i..];
    }
    out.push_str(rest);
    let mut text = String::new();
    let mut skip_until: Option<&str> = None;
    let mut s = out.as_str();
    while !s.is_empty() {
        if let Some(end) = skip_until {
            match s.find(end) {
                Some(e) => s = &s[e + end.len()..],
                None => break,
            }
            skip_until = None;
            continue;
        }
        match s.find('<') {
            Some(lt) => {
                text.push_str(&s[..lt]);
                text.push(' ');
                if s[lt..].starts_with("<script") {
                    skip_until = Some("</script>");
                } else if s[lt..].starts_with("<style") {
                    skip_until = Some("</style>");
                }
                s = s[lt..].find('>').map_or("", |gt| &s[lt + gt + 1..]);
            }
            None => {
                text.push_str(s);
                break;
            }
        }
    }
    text
}

/// The first `module.word` in `text` for a declared Module, e.g.
/// `kernel.backup.total`.
fn raw_key<'a>(text: &'a str, modules: &[&str]) -> Option<&'a str> {
    for m in modules {
        let needle = format!("{m}.");
        let mut from = 0;
        while let Some(at) = text[from..].find(&needle).map(|a| from + a) {
            let before = text[..at].chars().next_back();
            let after = text[at + needle.len()..].chars().next();
            let starts_word = !before.is_some_and(|c| c.is_ascii_alphanumeric() || c == '_');
            if starts_word && after.is_some_and(|c| c.is_ascii_lowercase()) {
                let end = text[at..]
                    .find(|c: char| !(c.is_ascii_alphanumeric() || c == '.' || c == '_'))
                    .map_or(text.len(), |e| at + e);
                return Some(&text[at..end]);
            }
            from = at + needle.len();
        }
    }
    None
}

/// `YYYY-MM-DD HH:MM` anywhere in `text`.
fn iso_time(text: &str) -> Option<&str> {
    let b = text.as_bytes();
    let d = |i: usize| b.get(i).is_some_and(u8::is_ascii_digit);
    (0..b.len()).find_map(|i| {
        let shape = (0..4).all(|k| d(i + k))
            && b.get(i + 4) == Some(&b'-')
            && d(i + 5)
            && d(i + 6)
            && b.get(i + 7) == Some(&b'-')
            && d(i + 8)
            && d(i + 9)
            && b.get(i + 10) == Some(&b' ')
            && d(i + 11)
            && d(i + 12)
            && b.get(i + 13) == Some(&b':');
        shape.then(|| &text[i..i + 16])
    })
}

/// Every back-office link of `body` worth reading: no downloads, no audit
/// entries (a record view showing the stored data), no edits of one text.
fn links(body: &str) -> Vec<String> {
    body.split("href=\"")
        .skip(1)
        .filter_map(|s| s.split('"').next())
        .filter(|p| p.starts_with("/admin"))
        .filter(|p| {
            ![
                ".csv",
                ".xlsx",
                "/file",
                "/download",
                "/export",
                "/audit/",
                "/texts/",
                "/logout",
            ]
            .iter()
            .any(|x| p.contains(x))
        })
        .map(|p| p.split('#').next().unwrap_or(p).replace("&amp;", "&"))
        .collect()
}

async fn admin<'a>(app: &'a TestApp) -> Browser<'a> {
    let b = app.browser();
    assert_eq!(b.login("admin@sealbay.example", PASSWORD).await.status, 303);
    b
}

#[tokio::test]
async fn no_main_page_shows_a_catalog_key_or_an_iso_time() {
    let h = Harness::new();
    let app = seeded(&h).await;
    let b = admin(&app).await;
    let mut modules: Vec<&str> = app
        .deployment()
        .permissions()
        .iter()
        .map(|p| p.id.split('.').next().unwrap())
        .collect();
    modules.extend(["contact", "my_info"]);
    modules.dedup();
    // The home, every page of the navigation, and the first page each of
    // those links to (a Student, an announcement, a roster import, …).
    let home = b.get("/admin").await;
    let mut todo = vec!["/admin".to_string()];
    todo.extend(links(&home.body));
    let mut seen: Vec<String> = Vec::new();
    let mut i = 0;
    while i < todo.len() && seen.len() < 60 {
        let path = todo[i].clone();
        i += 1;
        if seen.contains(&path) {
            continue;
        }
        let r = b.get(&path).await;
        assert_eq!(r.status, 200, "{path}");
        let text = visible(&r.body);
        assert_eq!(raw_key(&text, &modules), None, "{path}: {text}");
        assert_eq!(iso_time(&text), None, "{path}: {text}");
        if i <= 1 + links(&home.body).len() {
            let depth: Vec<String> = links(&r.body)
                .into_iter()
                .filter(|l| !todo.contains(l))
                .collect();
            todo.extend(depth.into_iter().take(3));
        }
        seen.push(path);
    }
    for must in [
        "/admin/roles",
        "/admin/texts",
        "/admin/usage",
        "/admin/backups",
        "/admin/staff",
    ] {
        assert!(seen.iter().any(|s| s == must), "{must} read: {seen:?}");
    }
    assert!(
        seen.iter().any(|s| s.starts_with("/admin/announcements/0")),
        "{seen:?}"
    );
    assert!(
        seen.iter().any(|s| s.starts_with("/admin/roster/0")),
        "{seen:?}"
    );
}

#[tokio::test]
async fn the_audience_column_reads_labels_not_keys() {
    let h = Harness::new();
    let app = seeded(&h).await;
    let b = admin(&app).await;
    let list = visible(&b.get("/admin/announcements").await.body);
    assert!(list.contains("ชั้นปี: 1"), "{list}");
    assert!(
        list.contains("หลักสูตร: วิทยาศาสตรมหาบัณฑิต (นานาชาติ)"),
        "{list}"
    );
    assert!(list.contains("ป้าย: ชมรมดนตรี"), "{list}");
    for raw in ["year:", "programme:", "(Tag)", "กลุ่ม (Tag)"] {
        assert!(!list.contains(raw), "{raw}: {list}");
    }
    // Thai dates with the Buddhist year.
    assert!(list.contains(" 2569 "), "{list}");
}

#[tokio::test]
async fn a_role_label_shows_and_the_stored_name_stays() {
    let h = Harness::new();
    let app = seeded(&h).await;
    let b = admin(&app).await;
    let staff = b.get("/admin/staff").await.body;
    let main = visible(&staff);
    assert!(main.contains("เจ้าหน้าที่กิจการนักศึกษา"), "{main}");
    assert!(main.contains("ผู้ดูแลระบบ"), "the Kernel's label: {main}");
    assert!(
        !main.contains("Officer") && !main.contains("Administrator"),
        "{main}"
    );
    // The form still posts the stored name.
    let new = b.get("/admin/staff/new").await.body;
    assert!(new.contains("name=\"role\" value=\"Officer\""), "{new}");
    assert!(visible(&new).contains("ผู้ดูข้อมูล"), "{new}");
    assert!(visible(&new).contains("ภาษา") && visible(&new).contains("ไทย"));
    // Roles keep their names in the database; the page shows them only in
    // the technical details.
    let names = h
        .db()
        .query(harpseal::Statement::new(
            "SELECT name FROM role ORDER BY name",
        ))
        .await
        .unwrap();
    let names: Vec<String> = (0..names.rows.len())
        .filter_map(|i| {
            names
                .get(i, "name")
                .and_then(|v| v.as_text())
                .map(str::to_string)
        })
        .collect();
    assert_eq!(names, ["Administrator", "Finance", "Officer", "Viewer"]);
    let roles = b.get("/admin/roles").await.body;
    let (main, tech) = roles.split_once("<details class=\"tech\">").unwrap();
    assert!(visible(main).contains("เจ้าหน้าที่การเงิน"));
    assert!(!visible(main).contains("Finance"));
    assert!(
        tech.contains("<span class=\"code\">Finance</span> เจ้าหน้าที่การเงิน"),
        "{tech}"
    );
}
