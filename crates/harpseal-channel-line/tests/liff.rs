//! Mini App (LIFF): launch URLs and ID token verification on the wire.

mod common;

use std::sync::Arc;

use common::{LineApi, SECRET, TOKEN, USER};
use harpseal::{Channel, Error, WebClient};
use harpseal_channel_line::{KIND, LINE_MINI_APP_BASE, LineChannel, LineConfig};

const LIFF: &str = "2000000000-SealBay0";

fn channel(liff: &str) -> (LineChannel, LineApi) {
    let api = LineApi::default();
    let ch = LineChannel::new(LineConfig::new(SECRET, TOKEN, liff), Arc::new(api.clone()));
    (ch, api)
}

fn claims(aud: &str) -> String {
    format!(
        r#"{{"iss":"https://access.line.me","sub":"{USER}","aud":"{aud}","exp":1793840400,"iat":1793836800,"name":"Seal Bay Student"}}"#
    )
}

#[tokio::test]
async fn no_liff_id_no_mini_app() {
    let (ch, _) = channel("");
    assert!(ch.mini_app().is_none());
    assert!(!ch.capabilities().mini_app);
}

#[tokio::test]
async fn launch_urls_open_the_liff_app() {
    let (ch, _) = channel(LIFF);
    let app = ch.mini_app().unwrap();
    assert_eq!(
        app.launch_url("/news/1042?from=menu"),
        "https://liff.line.me/2000000000-SealBay0/news/1042?from=menu"
    );
    assert_eq!(app.web_client(), Some(WebClient::Liff { id: LIFF.into() }));
}

#[tokio::test]
async fn a_line_mini_app_launches_under_its_own_base_with_the_same_sdk() {
    let cfg = LineConfig::new(SECRET, TOKEN, LIFF).with_launch_base(LINE_MINI_APP_BASE);
    let ch = LineChannel::new(cfg, Arc::new(LineApi::default()));
    let app = ch.mini_app().unwrap();
    assert_eq!(
        app.launch_url("/app"),
        "https://miniapp.line.me/2000000000-SealBay0/app"
    );
    assert_eq!(app.web_client(), Some(WebClient::Liff { id: LIFF.into() }));
}

#[tokio::test]
async fn a_launch_base_without_its_trailing_slash_gets_one() {
    let cfg = LineConfig::new(SECRET, TOKEN, LIFF).with_launch_base("https://miniapp.line.me");
    let ch = LineChannel::new(cfg, Arc::new(LineApi::default()));
    assert_eq!(
        ch.mini_app().unwrap().launch_url("/app"),
        "https://miniapp.line.me/2000000000-SealBay0/app"
    );
}

#[tokio::test]
async fn a_valid_id_token_resolves_to_the_line_user() {
    let (ch, api) = channel(LIFF);
    api.http().respond_with(200, claims("2000000000"));
    let who = ch
        .mini_app()
        .unwrap()
        .verify_session("eyJhbGciOi.eyJpc3Mi.c2ln")
        .await
        .unwrap();
    assert_eq!(who.channel, KIND);
    assert_eq!(who.external_id, USER);
    let reqs = api.take();
    assert_eq!(reqs[0].method, http::Method::POST);
    assert_eq!(reqs[0].uri, "https://api.line.me/oauth2/v2.1/verify");
    assert_eq!(
        reqs[0].headers["content-type"],
        "application/x-www-form-urlencoded"
    );
    assert!(
        reqs[0].headers.get("authorization").is_none(),
        "the Messaging API token never goes to LINE Login"
    );
    assert_eq!(
        reqs[0].body_text(),
        "id_token=eyJhbGciOi.eyJpc3Mi.c2ln&client_id=2000000000"
    );
}

#[tokio::test]
async fn refused_or_foreign_tokens_are_bad_signatures() {
    let (ch, api) = channel(LIFF);
    let app = ch.mini_app().unwrap();
    api.http().respond_with(
        400,
        r#"{"error":"invalid_request","error_description":"IdToken expired."}"#,
    );
    assert!(matches!(
        app.verify_session("t").await,
        Err(Error::BadSignature)
    ));
    api.http().respond_with(200, claims("1999999999"));
    assert!(matches!(
        app.verify_session("t").await,
        Err(Error::BadSignature)
    ));
    assert!(matches!(
        app.verify_session("").await,
        Err(Error::BadSignature)
    ));
    assert_eq!(
        api.take().len(),
        2,
        "an empty token is refused without a call"
    );
}

#[tokio::test]
async fn the_login_channel_id_can_be_set_and_paths_get_a_slash() {
    let api = LineApi::default();
    let cfg = LineConfig::new(SECRET, TOKEN, LIFF).with_login_channel_id("2000000099");
    let ch = LineChannel::new(cfg, Arc::new(api.clone()));
    let app = ch.mini_app().unwrap();
    assert_eq!(
        app.launch_url("news"),
        "https://liff.line.me/2000000000-SealBay0/news"
    );
    assert_eq!(
        app.launch_url("?p=1"),
        "https://liff.line.me/2000000000-SealBay0?p=1"
    );
    assert_eq!(
        app.launch_url(""),
        "https://liff.line.me/2000000000-SealBay0"
    );
    api.http().respond_with(200, claims("2000000099"));
    app.verify_session("t").await.unwrap();
    assert!(api.take()[0].body_text().ends_with("client_id=2000000099"));
}
