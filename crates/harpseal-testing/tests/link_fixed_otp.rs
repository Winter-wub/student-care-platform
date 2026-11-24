//! A fixed dev OTP (`EmailOtp::with_dev_fixed_code`, the native binary's
//! `HARPSEAL_DEV_FIXED_OTP`): every challenge gets the same code, which is
//! still hashed, emailed, expiring and attempt-counted; `build()` refuses
//! it outside dev mode. The env-var reader is tested in
//! `harpseal-runtime-native` (`dev_otp.rs`).

mod support;

use harpseal::{BuildError, EmailOtp, Statement, VerificationMethod};
use harpseal_channel_fake::FakeProfile;
use harpseal_testing::{Harness, TestApp};
use support::{Chat, Hello, ROSTER, boot, col, text_of};

const FIXED: &str = "123456";

fn fixed_otp() -> EmailOtp {
    EmailOtp::new(Harness::config().otp)
        .with_dev_fixed_code(FIXED)
        .unwrap()
}

async fn boot_fixed() -> (Harness, TestApp) {
    let h = Harness::new().with_fake_profile(FakeProfile::LineLike);
    // `h.builder()` is in dev mode; the second `.verification` replaces the first.
    let d = h
        .builder()
        .verification(fixed_otp())
        .module(Hello)
        .build()
        .unwrap();
    let app = h.boot(d).await;
    for sql in ROSTER {
        h.db().execute(Statement::new(*sql)).await.unwrap();
    }
    (h, app)
}

/// Follow, consent, student id: the OTP email has gone out.
async fn ask_code(c: &mut Chat<'_>, student_code: &str) {
    c.follow().await;
    c.press("kernel.consent.accept").await;
    c.text(student_code).await;
}

#[tokio::test]
async fn the_fixed_code_is_emailed_stored_hashed_and_links() {
    let (h, app) = boot_fixed().await;
    let mut c = Chat::new(&h, &app, "u1");
    ask_code(&mut c, "68010001").await;
    assert_eq!(c.code(), FIXED, "the email carries the fixed code");
    let row = app
        .query("SELECT secret_hash, expires_at, created_at FROM verification_challenge")
        .await;
    let hash = col(&row, "secret_hash").unwrap();
    assert!(
        hash.starts_with("sha256$") && !hash.contains(FIXED),
        "{hash}"
    );
    let ttl = row.get(0, "expires_at").unwrap().as_i64().unwrap()
        - row.get(0, "created_at").unwrap().as_i64().unwrap();
    assert_eq!(ttl, i64::from(Harness::config().otp.ttl_minutes) * 60_000);

    let seen = c.text(FIXED).await;
    assert!(text_of(&seen[0]).starts_with("เชื่อมต่อสำเร็จ"), "{seen:?}");
    assert_eq!(
        app.count("SELECT count(*) FROM link WHERE status = 'active'")
            .await,
        1
    );

    // A second Student gets the same code, under a different salt.
    let mut c4 = Chat::new(&h, &app, "u4");
    ask_code(&mut c4, "68010004").await;
    assert_eq!(c4.code(), FIXED);
    let hashes = app
        .query("SELECT DISTINCT secret_hash FROM verification_challenge")
        .await;
    assert_eq!(hashes.len(), 2, "a fresh salt per challenge");
}

#[tokio::test]
async fn a_wrong_code_still_counts_attempts_and_locks() {
    let (h, app) = boot_fixed().await;
    let mut c = Chat::new(&h, &app, "u1");
    ask_code(&mut c, "68010001").await;
    let seen = c.text("000000").await;
    assert!(text_of(&seen[0]).starts_with("รหัสไม่ถูกต้อง"), "{seen:?}");
    let row = app
        .query("SELECT attempts, state FROM verification_challenge")
        .await;
    assert_eq!(row.get(0, "attempts").unwrap().as_i64(), Some(1));
    assert_eq!(col(&row, "state").as_deref(), Some("pending"));

    // The attempt limit still applies: the last wrong answer locks.
    let max = Harness::config().otp.max_attempts;
    for _ in 1..max {
        c.text("000000").await;
    }
    assert_eq!(
        app.count("SELECT count(*) FROM security_event WHERE kind = 'otp.lockout'")
            .await,
        1
    );
    // Locked: even the fixed code no longer links.
    c.text(FIXED).await;
    assert_eq!(app.count("SELECT count(*) FROM link").await, 0);
}

#[tokio::test]
async fn build_refuses_a_fixed_code_outside_dev_mode() {
    let h = Harness::new();
    let err = h
        .builder()
        .dev(false)
        .verification(fixed_otp())
        .module(Hello)
        .build()
        .err()
        .expect("refused");
    assert_eq!(err, BuildError::DevOnlyVerification("email_otp"));
    assert!(err.to_string().contains("not in dev mode"), "{err}");
    // The plain method builds either way.
    assert!(
        h.builder()
            .dev(false)
            .verification(EmailOtp::new(Harness::config().otp))
            .module(Hello)
            .build()
            .is_ok()
    );
}

#[test]
fn only_a_code_of_the_configured_digits_is_accepted() {
    let otp = || EmailOtp::new(Harness::config().otp);
    for bad in ["12345", "1234567", "12345a", "", "１２３４５６"] {
        assert!(otp().with_dev_fixed_code(bad).is_err(), "{bad:?}");
    }
    assert!(otp().with_dev_fixed_code(FIXED).unwrap().dev_only());
    assert!(!otp().dev_only());
    let shown = format!("{:?}", fixed_otp());
    assert!(!shown.contains(FIXED), "Debug hides the code: {shown}");
}

#[tokio::test]
async fn without_a_fixed_code_the_otp_is_random() {
    let (h, app) = boot(FakeProfile::LineLike).await;
    let mut c = Chat::new(&h, &app, "u1");
    ask_code(&mut c, "68010001").await;
    let first = c.code();
    assert_eq!(first.len(), 6);
    assert_ne!(first, FIXED);
    let mut c4 = Chat::new(&h, &app, "u4");
    ask_code(&mut c4, "68010004").await;
    assert_ne!(c4.code(), first, "each challenge draws its own code");
}
