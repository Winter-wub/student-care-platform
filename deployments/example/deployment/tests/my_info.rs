//! The example's "My info" Module (#72): a Student opens it from the Mini
//! App home and sees only their own record.

use example_deployment::{Secrets, builder, my_info};
use harpseal::Statement;
use harpseal_testing::{Browser, Harness, TestApp};

fn secrets() -> Secrets {
    Secrets::from_lookup(|k| match k {
        "LINE_LIFF_ID" => Some("1234567890-AbCdEfGh".into()),
        "TELEGRAM_WEB_APP_URL" => Some("https://app.seal-bay.invalid/tg".into()),
        _ => None,
    })
}

async fn run(h: &Harness, sql: &str) {
    h.db()
        .execute(Statement::new(sql.to_string()))
        .await
        .unwrap();
}

/// Two linked Students of Seal Bay University on the Fake Channel.
async fn boot(h: &Harness) -> TestApp {
    let d = builder(h.services(), secrets())
        .unwrap()
        .channel(h.fake_channel())
        .build()
        .expect("the example Deployment builds");
    let app = h.boot(d).await;
    for (id, code, name, attrs) in [
        (
            "s1",
            "68010001",
            "Karn Seal",
            r#"{"programme":"MSC","year":2,"gpa":3.41}"#,
        ),
        (
            "s2",
            "68010002",
            "Mali Tern",
            r#"{"programme":"MSC-Intl","year":4}"#,
        ),
    ] {
        run(
            h,
            &format!(
                "INSERT INTO student (id, student_code, display_name, locale, attributes, \
                 created_at, updated_at) VALUES ('{id}', '{code}', '{name}', 'th', '{attrs}', 1, 1)"
            ),
        )
        .await;
        run(
            h,
            &format!(
                "INSERT INTO channel_account (id, channel, external_id, first_seen_at, \
                 flow_state, updated_at) VALUES ('a-{id}', 'fake', 'u-{id}', 1, 'linked', 1)"
            ),
        )
        .await;
        run(
            h,
            &format!(
                "INSERT INTO link (id, student_id, channel_account_id, channel, status, method, \
                 linked_at, updated_at) VALUES ('l-{id}', '{id}', 'a-{id}', 'fake', 'active', \
                 'email_otp', 1, 1)"
            ),
        )
        .await;
    }
    app
}

async fn open<'a>(h: &Harness, app: &'a TestApp, student: &str) -> Browser<'a> {
    let b = app.browser();
    let r = b
        .open_mini_app(&h.fake_channel(), &format!("u-{student}"), my_info::PATH)
        .await;
    assert_eq!(r.status, 303, "{}", r.body);
    b
}

#[tokio::test]
async fn the_home_links_to_my_info_which_shows_my_record() {
    let h = Harness::new();
    let app = boot(&h).await;
    let b = open(&h, &app, "s1").await;
    let home = b.get("/app").await.body;
    assert!(
        home.contains("<a href=\"/app/my-info\">ข้อมูลของฉัน</a>"),
        "{home}"
    );
    let r = b.get(my_info::PATH).await;
    assert_eq!(r.status, 200);
    for seen in [
        "<dd>Karn Seal</dd>",
        "<dd>68010001</dd>",
        "<dt>หลักสูตร</dt><dd>MSC</dd>",
        "<dt>ชั้นปี</dt><dd>2</dd>",
    ] {
        assert!(r.body.contains(seen), "{seen}");
    }
    // The GPA is not `student_visible`: neither its label nor its value.
    assert!(!r.body.contains("เกรดเฉลี่ย") && !r.body.contains("3.41"));
    assert!(r.header("content-security-policy").is_some());
}

#[tokio::test]
async fn a_student_never_sees_anothers_record_through_the_url() {
    let h = Harness::new();
    let app = boot(&h).await;
    let karn = open(&h, &app, "s1").await;
    for path in [
        "/app/my-info?student=s2",
        "/app/my-info?student_code=68010002",
    ] {
        let r = karn.get(path).await;
        assert!(r.body.contains("Karn Seal"), "{path}");
        assert!(
            !r.body.contains("Mali Tern") && !r.body.contains("68010002"),
            "{path}"
        );
    }
    assert_eq!(karn.get("/app/my-info/s2").await.status, 404);
    // Without a session: the entry page, which signs in back to My info.
    let r = app.get("/app/my-info?student=s2").await;
    assert!(r.body.contains("name=\"next\" value=\"/app/my-info\""));
    assert!(!r.body.contains("Mali Tern"));
    // Mali's own session shows Mali only.
    let mali = open(&h, &app, "s2").await;
    let r = mali.get(my_info::PATH).await;
    assert!(r.body.contains("Mali Tern") && !r.body.contains("Karn Seal"));
}
