//! Secrets come from a lookup (env / Worker secrets), never from TOML.

use example_deployment::{Secrets, StartError, deployment};
use harpseal_testing::Harness;

#[test]
fn secrets_read_every_variable_through_the_lookup() {
    let asked = std::cell::RefCell::new(Vec::new());
    let s = Secrets::from_lookup(|k| {
        asked.borrow_mut().push(k.to_string());
        Some(format!("value-of-{k}"))
    });
    assert_eq!(s.fake_secret, "value-of-FAKE_SECRET");
    assert_eq!(*asked.borrow(), Secrets::VARS);
    assert!(!example_deployment::DEPLOYMENT_TOML.contains("FAKE_SECRET ="));
}

#[test]
fn missing_fake_secret_refuses_to_start() {
    let h = Harness::new();
    let err = deployment(h.services(), Secrets::from_lookup(|_| None)).unwrap_err();
    assert!(matches!(err, StartError::FakeSecret(_)), "{err}");
}

#[test]
fn an_invalid_public_url_refuses_to_start() {
    let h = Harness::new();
    let bad = |v: &str| {
        let mut s = Secrets::from_lookup(|_| None);
        s.fake_secret = "s3cret-s3cret-s3cret".into();
        s.public_url = v.into();
        s
    };
    for v in [
        "http://students.sealbay.example",
        "https://students.sealbay.example/app",
    ] {
        let err = example_deployment::builder(h.services(), bad(v)).err();
        assert!(matches!(err, Some(StartError::PublicUrl(_))), "{v}");
        let err = deployment(h.services(), bad(v)).unwrap_err();
        assert!(matches!(err, StartError::PublicUrl(_)), "{v}: {err}");
    }
    // Set and valid: it wins over the TOML.
    let ok = bad("https://students.sealbay.example/");
    assert!(example_deployment::builder(h.services(), ok).is_ok());
}
