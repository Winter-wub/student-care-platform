//! A Module's own Mini App page (#72) at the HTTP seam: the page's
//! handler takes `AppStudent`, the Module lists it on the Mini App home
//! with `mini_app_link`. Setup: `app_module_support`; `POST`s:
//! `app_module_post.rs`.

mod app_module_support;

use axum::routing::get;
use harpseal::{BuildError, Module, ModuleRegistrar};
use harpseal_testing::Harness;

use app_module_support::{
    INIT_DATA, PAGE, Probe, RAW, TEXTS, boot, builder, is_entry, probe, run, signed_in, who,
};

#[tokio::test]
async fn line_and_telegram_sessions_reach_their_own_student() {
    let (h, app) = boot().await;
    let line = signed_in(&h, &app, "line").await;
    let r = line.get(PAGE).await;
    assert_eq!(r.status, 200);
    // Only `student_visible` attributes: not `advisor`, not the stale
    // `legacy` key.
    assert_eq!(who(&r), "s1|6801001|Karn Seal|th|line|{&quot;year&quot;:1}");
    assert!(r.body.contains("หน้าทดสอบ"), "Thai texts for Karn");
    // The Mini App's headers: CSP, never cached, no referrer.
    assert_eq!(
        r.header("content-security-policy"),
        Some("frame-ancestors 'self' https://web.telegram.org")
    );
    assert_eq!(r.header("cache-control"), Some("no-store"));
    assert_eq!(r.header("referrer-policy"), Some("no-referrer"));

    let tg = signed_in(&h, &app, "telegram").await;
    let r = tg.get(PAGE).await;
    assert_eq!(
        who(&r),
        "s2|6801002|Mali Tern|en|telegram|{&quot;year&quot;:3}"
    );
    assert!(r.body.contains("Probe page"), "English texts for Mali");
}

#[tokio::test]
async fn telegram_web_s_partitioned_cookie_reaches_the_module_page() {
    let (_h, app) = boot().await;
    let req = http::Request::post("/app/session")
        .header("content-type", "application/x-www-form-urlencoded")
        .body(axum::body::Body::from(format!(
            "channel=telegram&token={}&next=%2Fapp%2Fprobe&framed=1",
            harpseal_testing::urlencode(INIT_DATA)
        )))
        .unwrap();
    let r = app.request(req).await;
    let cookie = r.header("set-cookie").unwrap().to_string();
    assert!(cookie.contains("Path=/app;") && cookie.contains("Partitioned"));
    let token = cookie.split(';').next().unwrap();
    let req = http::Request::get(PAGE)
        .header("cookie", token)
        .body(axum::body::Body::empty())
        .unwrap();
    assert!(who(&app.request(req).await).starts_with("s2|"));
}

#[tokio::test]
async fn without_a_session_the_page_is_the_entry_page_back_to_itself() {
    let (_h, app) = boot().await;
    let r = app.get(PAGE).await;
    assert!(is_entry(&r), "{}", r.body);
    assert!(r.body.contains("name=\"next\" value=\"/app/probe\""));
    assert!(r.header("content-security-policy").is_some());
    // A forged cookie: the same, and the cookie is cleared.
    let req = http::Request::get(PAGE)
        .header("cookie", "app_session=00ff00ff")
        .body(axum::body::Body::empty())
        .unwrap();
    let r = app.request(req).await;
    assert!(is_entry(&r));
    assert!(r.header("set-cookie").unwrap().contains("Max-Age=0"));
}

#[tokio::test]
async fn an_expired_session_goes_back_through_the_sign_in() {
    let (h, app) = boot().await;
    for channel in ["line", "telegram"] {
        let b = signed_in(&h, &app, channel).await;
        h.clock().advance_minutes(61);
        let r = b.get(PAGE).await;
        assert!(is_entry(&r), "{channel}: {}", r.body);
        assert!(!r.body.contains("id=\"who\""));
        h.clock().advance_minutes(-61);
    }
}

#[tokio::test]
async fn a_revoked_link_gets_the_not_linked_page() {
    let (h, app) = boot().await;
    let b = signed_in(&h, &app, "line").await;
    run(&h, "UPDATE link SET status = 'revoked' WHERE id = 'l-line'").await;
    let r = b.get(PAGE).await;
    assert_eq!(r.status, 200);
    assert!(r.body.contains("ยังไม่ได้เชื่อมกับรหัสนักศึกษา"), "{}", r.body);
    assert!(!r.body.contains("Karn Seal"));
}

#[tokio::test]
async fn no_url_or_query_picks_another_student_only_the_cookie_does() {
    let (h, app) = boot().await;
    let karn = signed_in(&h, &app, "line").await;
    for path in [
        "/app/probe?student=s2",
        "/app/probe?id=s2&student_code=6801002",
        "/app/probe/first?student=s2",
    ] {
        let r = karn.get(path).await;
        assert!(who(&r).starts_with("s1|"), "{path}");
        assert!(!r.body.contains("Mali Tern"), "{path}");
    }
    assert_eq!(karn.get("/app/probe/s2").await.status, 404);
    // Mali's cookie is Mali: the page follows the session, never mixes.
    let mali = signed_in(&h, &app, "telegram").await;
    let cookie = mali.cookie("app_session").unwrap();
    let req = http::Request::get("/app/probe?student=s1")
        .header("cookie", format!("app_session={cookie}"))
        .body(axum::body::Body::empty())
        .unwrap();
    let r = app.request(req).await;
    assert!(who(&r).starts_with("s2|") && !r.body.contains("Karn Seal"));
}

#[tokio::test]
async fn the_home_lists_module_links_in_order() {
    let (h, app) = boot().await;
    let b = signed_in(&h, &app, "line").await;
    let home = b.get("/app").await.body;
    let first = home.find("href=\"/app/probe/first\"").expect("first link");
    let page = home.find("href=\"/app/probe\"").expect("page link");
    assert!(first < page, "order 20 before 30");
    assert!(home.contains(">ลิงก์แรก</a>") && home.contains(">หน้าทดสอบ</a>"));
    // Mali reads them in English.
    let tg = signed_in(&h, &app, "telegram").await;
    assert!(tg.get("/app").await.body.contains(">First link</a>"));
}

struct Stray(&'static str, &'static str);

impl Module for Stray {
    fn name(&self) -> &'static str {
        "stray"
    }
    fn register(&self, r: &mut ModuleRegistrar) {
        r.messages(TEXTS)
            .public(self.0, get(probe))
            .mini_app_link("probe.page", self.1, 1);
    }
}

#[test]
fn build_refuses_a_link_outside_app_or_to_a_page_it_does_not_own() {
    for (route, link) in [
        ("/my-info", "/my-info"),
        ("/app/stray", "/app/elsewhere"),
        ("/app/stray", PAGE),
        ("/app/", "/app/"),
    ] {
        let h = Harness::new();
        let err = builder(&h)
            .module(Probe)
            .module(Stray(route, link))
            .build()
            .unwrap_err();
        assert!(
            matches!(
                err,
                BuildError::InvalidRoute {
                    module: "stray",
                    ..
                }
            ),
            "{link}: {err}"
        );
    }
}

#[tokio::test]
async fn every_app_response_is_no_store_even_without_the_page_shell() {
    let (h, app) = boot().await;
    let b = signed_in(&h, &app, "line").await;
    let r = b.get(RAW).await;
    assert_eq!(r.status, 200);
    assert_eq!(r.body, "s1");
    assert_eq!(r.header("cache-control"), Some("no-store"));
    assert!(r.header("content-security-policy").is_some());
}
