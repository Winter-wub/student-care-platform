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
