//! EmailSender tests: SMTP against an in-process mock SMTP server, Resend
//! against the harness's recording HttpClient.

use std::sync::Arc;

use harpseal::{Email, EmailSender, ResendEmail};
use harpseal_runtime_native::Smtp;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpListener;

/// What the mock server received in one session.
#[derive(Debug, Default)]
struct Session {
    mail_from: String,
    rcpt_to: Vec<String>,
    data: String,
}

/// Accepts one connection and speaks just enough SMTP (RFC 5321) for one
/// message: greeting, EHLO, MAIL, RCPT, DATA, QUIT.
async fn mock_smtp() -> (u16, tokio::task::JoinHandle<Session>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let task = tokio::spawn(async move {
        let (sock, _) = listener.accept().await.unwrap();
        let (r, mut w) = sock.into_split();
        let mut lines = BufReader::new(r).lines();
        let mut s = Session::default();
        w.write_all(b"220 mock.sealbay.example ESMTP\r\n")
            .await
            .unwrap();
        while let Some(line) = lines.next_line().await.unwrap() {
            let upper = line.to_ascii_uppercase();
            let reply: &[u8] = if upper.starts_with("EHLO") || upper.starts_with("HELO") {
                b"250-mock.sealbay.example\r\n250 8BITMIME\r\n"
            } else if upper.starts_with("MAIL FROM:") {
                s.mail_from = line[10..].trim().to_string();
                b"250 OK\r\n"
            } else if upper.starts_with("RCPT TO:") {
                s.rcpt_to.push(line[8..].trim().to_string());
                b"250 OK\r\n"
            } else if upper == "DATA" {
                w.write_all(b"354 end with <CRLF>.<CRLF>\r\n")
                    .await
                    .unwrap();
                while let Some(l) = lines.next_line().await.unwrap() {
                    if l == "." {
                        break;
                    }
                    s.data.push_str(&l);
                    s.data.push('\n');
                }
                b"250 OK queued\r\n"
            } else if upper == "QUIT" {
                w.write_all(b"221 bye\r\n").await.unwrap();
                break;
            } else {
                b"250 OK\r\n"
            };
            w.write_all(reply).await.unwrap();
        }
        s
    });
    (port, task)
}

fn otp_email() -> Email {
    Email {
        to: "student68010001@sealbay.example".into(),
        subject: "Your Seal Bay University code".into(),
        text: "Your code is 482913. It expires in 5 minutes.".into(),
        html: None,
    }
}

#[tokio::test]
async fn smtp_sends_envelope_and_body_to_the_relay() {
    let (port, server) = mock_smtp().await;
    let smtp = Smtp::new(
        &format!("smtp://127.0.0.1:{port}"),
        "Seal Bay University <noreply@sealbay.example>",
    )
    .unwrap();
    smtp.send(otp_email()).await.unwrap();
    drop(smtp); // closes the pooled connection (QUIT)
    let s = tokio::time::timeout(std::time::Duration::from_secs(5), server)
        .await
        .expect("mock SMTP session ended")
        .unwrap();

    assert!(
        s.mail_from.starts_with("<noreply@sealbay.example>"),
        "{s:?}"
    );
    assert_eq!(s.rcpt_to, ["<student68010001@sealbay.example>"]);
    assert!(
        s.data.contains("Subject: Your Seal Bay University code"),
        "{}",
        s.data
    );
    assert!(s.data.contains("To: student68010001@sealbay.example"));
    let from = s.data.lines().find(|l| l.starts_with("From: ")).unwrap();
    assert!(
        from.contains("Seal Bay University") && from.contains("<noreply@sealbay.example>"),
        "{from}"
    );
    assert!(s.data.contains("Your code is 482913."), "{}", s.data);
}

#[tokio::test]
async fn smtp_rejects_bad_settings_and_addresses() {
    assert!(Smtp::new("ftp://nowhere", "a@b.example").is_err());
    assert!(Smtp::new("smtp://localhost:25", "not an address").is_err());
    let smtp = Smtp::new("smtp://127.0.0.1:9", "a@b.example").unwrap();
    let mut bad = otp_email();
    bad.to = "nobody".into();
    let err = smtp.send(bad).await.unwrap_err();
    assert!(err.to_string().contains("recipient"), "{err}");
}

#[tokio::test]
async fn resend_posts_json_with_bearer_key() {
    let http = harpseal_testing::RecordingHttp::default();
    let resend = ResendEmail::new(
        Arc::new(http.clone()),
        "re_test_key",
        "Seal Bay University <noreply@sealbay.example>",
    );
    http.respond_with(200, r#"{"id":"4ef9a417"}"#);
    let mut email = otp_email();
    email.html = Some("<p>Your code is <b>482913</b>.</p>".into());
    resend.send(email).await.unwrap();

    let reqs = http.requests();
    assert_eq!(reqs.len(), 1);
    let r = &reqs[0];
    assert_eq!(r.method, http::Method::POST);
    assert_eq!(r.uri, "https://api.resend.com/emails");
    assert_eq!(r.headers["authorization"], "Bearer re_test_key");
    assert_eq!(r.headers["content-type"], "application/json");
    assert_eq!(
        r.body_text(),
        r#"{"from":"Seal Bay University <noreply@sealbay.example>","to":["student68010001@sealbay.example"],"subject":"Your Seal Bay University code","text":"Your code is 482913. It expires in 5 minutes.","html":"<p>Your code is <b>482913</b>.</p>"}"#
    );
}

#[tokio::test]
async fn resend_error_answer_is_an_error() {
    let http = harpseal_testing::RecordingHttp::default();
    let resend = ResendEmail::new(Arc::new(http.clone()), "bad", "noreply@sealbay.example");
    http.respond_with(
        403,
        r#"{"name":"validation_error","message":"domain not verified"}"#,
    );
    let err = resend.send(otp_email()).await.unwrap_err();
    assert!(err.to_string().contains("403"), "{err}");
    assert!(err.to_string().contains("domain not verified"), "{err}");
}
