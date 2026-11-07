//! Mini App (#86), what a launch check alone does not catch: the idle
//! lifetime (Back, reload, a reopened tab), the per-tab stamp (two tabs
//! share one cookie), `?launch`, and the other `Sec-Fetch-Site` values.
//! The page scripts themselves run only in a browser; here the server's
//! side of them is checked.

mod app_module_support;

use app_module_support::*;
use harpseal_testing::TestResponse;

fn get_with(site: &str) -> http::Request<axum::body::Body> {
    http::Request::get(PAGE)
        .header("sec-fetch-site", site)
        .body(axum::body::Body::empty())
        .unwrap()
}

/// The tab stamp the page's first script carries.
fn stamp(r: &TestResponse) -> String {
    let key = "var p = '";
    let at = r.body.find(key).expect("the tab script") + key.len();
    r.body[at..at + 16].to_string()
}

#[tokio::test]
async fn none_and_same_site_are_new_launches_too() {
    let (h, app) = boot().await;
    // `none`: the chat app opened the URL, or it was typed or bookmarked;
    // `same-site`: a sibling subdomain.
    for site in ["none", "same-site"] {
        let b = signed_in(&h, &app, "line").await;
        let r = b.send(get_with(site)).await;
        assert!(is_entry(&r), "{site}: {}", r.body);
        assert!(!r.body.contains("Karn Seal"), "{site}");
        assert!(b.cookie("app_session").is_none(), "{site}");
    }
}

#[tokio::test]
async fn launch_in_the_query_is_a_new_launch_even_from_our_own_page() {
    let (h, app) = boot().await;
    let b = signed_in(&h, &app, "line").await;
    for path in [format!("{PAGE}?launch"), "/app?launch=1".to_string()] {
        let b2 = signed_in(&h, &app, "line").await;
        let r = b2.get(&path).await;
        assert!(is_entry(&r), "{path}: {}", r.body);
        // `next` is the path alone: after the exchange, no loop.
        assert!(!r.body.contains("value=\"/app/probe?launch"), "{path}");
    }
    assert!(who(&b.get(&format!("{PAGE}?launched=1")).await).starts_with("s1|"));
}

#[tokio::test]
async fn a_session_ends_after_fifteen_idle_minutes_and_an_hour_at_most() {
    let (h, app) = boot().await;
    let b = signed_in(&h, &app, "line").await;
    // Used every 10 minutes: alive, until the hour from the exchange.
    for _ in 0..5 {
        h.clock().advance_minutes(10);
        assert!(who(&b.get(PAGE).await).starts_with("s1|"));
    }
    h.clock().advance_minutes(10);
    assert!(is_entry(&b.get(PAGE).await), "one hour at most");
    // Back, a reload or a reopened tab 16 minutes later: the entry page.
    let b = signed_in(&h, &app, "line").await;
    h.clock().advance_minutes(14);
    assert!(who(&b.get(PAGE).await).starts_with("s1|"));
    h.clock().advance_minutes(16);
    assert!(is_entry(&b.get(PAGE).await), "idle for 16 minutes");
}

#[tokio::test]
async fn idle_minutes_is_configurable_within_an_hour() {
    for bad in ["0", "61"] {
        let toml = format!(
            "{}\n[mini_app]\nidle_minutes = {bad}\n",
            harpseal_testing::TEST_CONFIG
        );
        assert!(
            harpseal::DeploymentConfig::from_toml(&toml).is_err(),
            "{bad}"
        );
    }
    let toml = format!(
        "{}\n[mini_app]\nidle_minutes = 5\n",
        harpseal_testing::TEST_CONFIG
    );
    let c = harpseal::DeploymentConfig::from_toml(&toml).unwrap();
    assert_eq!(c.mini_app.idle_minutes, 5);
}

#[tokio::test]
async fn each_session_page_carries_its_own_tab_stamp_before_any_content() {
    let (h, app) = boot().await;
    let karn = signed_in(&h, &app, "line").await;
    let mali = signed_in(&h, &app, "telegram").await;
    let k = karn.get(PAGE).await;
    let m = mali.get(PAGE).await;
    assert_ne!(stamp(&k), stamp(&m), "another session, another stamp");
    assert_eq!(stamp(&k), stamp(&karn.get("/app").await), "per session");
    // The script runs before the Student's data is parsed or painted.
    let script = k.body.find("hs_app_s").unwrap();
    assert!(script < k.body.find("id=\"who\"").unwrap());
    assert!(k.body.contains("?launch") && k.body.contains("e.persisted"));
    // The stamp says nothing usable: not the CSRF token, not the cookie.
    let csrf = karn.csrf().unwrap();
    assert!(!csrf.starts_with(&stamp(&k)));
    assert!(!karn.cookie("app_session").unwrap().starts_with(&stamp(&k)));
    // The entry page marks the tab before it exchanges.
    let entry = app.get(PAGE).await;
    assert!(entry.body.contains("setItem('hs_app_s', 'pending')"));
}

#[tokio::test]
async fn the_signed_out_page_closes_the_mini_app_where_it_can() {
    let (h, app) = boot().await;
    let b = signed_in(&h, &app, "telegram").await;
    b.get(PAGE).await;
    let r = b.post("/app/sign-out", "").await;
    assert_eq!(r.status, 200, "{}", r.body);
    for part in [
        "tg.close()",
        "lf.closeWindow()",
        "data-liff=\"2000000000-SealBay0\"",
    ] {
        assert!(r.body.contains(part), "{part}: {}", r.body);
    }
}
