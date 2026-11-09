//! The back office's markup on the seeded example (#129): on the home,
//! every page of the navigation, the first pages those link to, the
//! announcement editor and a roster import refused for its headers, one
//! `h1` and no skipped heading level, every field labelled, and one
//! message announced per update (`harpseal_testing::a11y`).

use axum::body::Body;
use example_deployment::{Secrets, builder, seed::seed};
use harpseal_testing::{Browser, Harness, TestApp, a11y};
use http::{Request, header};

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
    seed(app.deployment(), Some("admin@sealbay.example"), false)
        .await
        .expect("seeds");
    app
}

/// The back-office links of `body` that show a page.
fn links(body: &str) -> Vec<String> {
    body.split("href=\"")
        .skip(1)
        .filter_map(|s| s.split('"').next())
        .filter(|p| p.starts_with("/admin"))
        .filter(|p| {
            ![".csv", ".xlsx", "/file", "/download", "/export", "/logout"]
                .iter()
                .any(|x| p.contains(x))
        })
        .map(|p| p.split('#').next().unwrap_or(p).replace("&amp;", "&"))
        .collect()
}

/// A roster file whose header row matches nothing, uploaded as the form
/// sends it.
async fn upload_wrong_headers(b: &Browser<'_>) -> String {
    let boundary = "----sealbay-a11y";
    let token = b.csrf().expect("a page gave a token");
    let body = format!(
        "--{boundary}\r\nContent-Disposition: form-data; name=\"_csrf\"\r\n\r\n{token}\r\n\
         --{boundary}\r\nContent-Disposition: form-data; name=\"mode\"\r\n\r\nupsert\r\n\
         --{boundary}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"wrong.csv\"\r\n\
         Content-Type: text/csv\r\n\r\nid,name\r\n65010001,ทดสอบ\r\n\r\n--{boundary}--\r\n"
    );
    let req = Request::post("/admin/roster/upload")
        .header(
            header::CONTENT_TYPE,
            format!("multipart/form-data; boundary={boundary}"),
        )
        .body(Body::from(body))
        .unwrap();
    let r = b.send(req).await;
    assert_eq!(r.status, 303, "{}", r.body);
    r.header("location").expect("to the import").to_string()
}

#[tokio::test]
async fn back_office_pages_keep_heading_order_labels_and_one_message() {
    let h = Harness::new();
    let app = seeded(&h).await;
    let b = app.browser();
    assert_eq!(b.login("admin@sealbay.example", PASSWORD).await.status, 303);

    let home = b.get("/admin").await;
    let nav = links(&home.body);
    let mut todo = vec!["/admin".to_string(), "/admin/announcements/new".to_string()];
    todo.extend(nav.iter().cloned());
    let mut seen: Vec<String> = Vec::new();
    let mut found: Vec<String> = Vec::new();
    let mut i = 0;
    while i < todo.len() && seen.len() < 70 {
        let path = todo[i].clone();
        i += 1;
        if seen.contains(&path) {
            continue;
        }
        let r = b.get(&path).await;
        assert_eq!(r.status, 200, "{path}");
        found.extend(
            a11y::problems(&r.body)
                .into_iter()
                .map(|p| format!("{path}: {p}")),
        );
        // One level deeper than the navigation: a few links of each page.
        if nav.contains(&path) {
            let deeper: Vec<String> = links(&r.body)
                .into_iter()
                .filter(|l| !todo.contains(l))
                .collect();
            todo.extend(deeper.into_iter().take(3));
        }
        seen.push(path);
    }

    // The roster import refused for its headers (it went h1 to h4, and
    // announced its refusal twice).
    b.get("/admin/roster").await;
    let at = upload_wrong_headers(&b).await;
    let r = b.get(&at).await;
    assert_eq!(r.status, 200, "{at}");
    assert!(r.body.contains("id=\"expected-headers\""), "{}", r.body);
    found.extend(
        a11y::problems(&r.body)
            .into_iter()
            .map(|p| format!("{at}: {p}")),
    );
    seen.push(at);

    for must in [
        "/admin/students",
        "/admin/roster",
        "/admin/announcements",
        "/admin/announcements/new",
        "/admin/usage",
        "/admin/roles",
        "/admin/fees",
    ] {
        assert!(seen.iter().any(|s| s == must), "{must} read: {seen:?}");
    }
    assert!(seen.len() >= 30, "{seen:?}");
    assert!(found.is_empty(), "{}", found.join("\n"));
}
