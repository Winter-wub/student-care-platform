//! The CLI library against an in-memory SQLite Deployment (#21).

use harpseal_cli::{CliError, Command, PasswordSource, StaffArgs, create_staff, parse, run};
use harpseal_testing::Harness;

fn args(s: &str) -> Vec<String> {
    s.split_whitespace().map(str::to_string).collect()
}

fn admin(email: &str) -> StaffArgs {
    let Command::CreateStaff(a) =
        parse(&args(&format!("staff create --admin --email {email}"))).unwrap()
    else {
        panic!("staff create");
    };
    a
}

#[tokio::test]
async fn creates_the_first_administrator_on_an_empty_database() {
    let h = Harness::new();
    // No `[roles]`: the database starts without any Role.
    let toml = "name = \"Seal Bay University (test)\"\ndefault_locale = \"th\"";
    let config = harpseal::DeploymentConfig::from_toml(toml).unwrap();
    let d = h.builder().config(config).build().unwrap();
    let c = create_staff(&d, &admin("root@sealbay.example"), None)
        .await
        .unwrap();
    assert!(c.created_admin_role);
    let temp = c.temporary_password.clone().expect("a generated password");
    assert_eq!(temp.len(), 27);

    // The Administrator Role holds every declared Permission.
    let app = h.boot(d).await;
    let perms = app
        .count(
            "SELECT count(*) FROM role_permission rp JOIN role r ON r.id = rp.role_id \
             WHERE r.name = 'Administrator'",
        )
        .await;
    assert_eq!(perms as usize, app.deployment().permissions().len());

    // The temporary password signs in and must be changed first.
    let b = app.browser();
    let r = b.login("root@sealbay.example", &temp).await;
    assert_eq!(r.header("location"), Some("/admin/password"));
}

#[tokio::test]
async fn refuses_a_second_administrator_without_force() {
    let h = Harness::new();
    let d = h.builder().build().unwrap();
    let mut first = admin("root@sealbay.example");
    first.password = PasswordSource::Given("a long enough password".into());
    let c = create_staff(&d, &first, Some("a long enough password".into()))
        .await
        .unwrap();
    assert_eq!(c.temporary_password, None);

    let second = admin("other@sealbay.example");
    let err = create_staff(&d, &second, None).await.unwrap_err();
    assert!(matches!(err, CliError::Refused(_)), "{err}");

    let mut forced = second.clone();
    forced.force = true;
    let c = create_staff(&d, &forced, None).await.unwrap();
    assert!(!c.created_admin_role, "the Role already exists");
    let app = h.boot(d).await;
    assert_eq!(app.count("SELECT count(*) FROM staff").await, 2);
    assert_eq!(
        app.count("SELECT count(*) FROM audit_log WHERE action = 'staff.create'")
            .await,
        2
    );
}

#[tokio::test]
async fn run_migrates_and_reads_the_password_from_stdin() {
    let h = Harness::new();
    let d = h.builder().build().unwrap();
    let mut out = Vec::new();
    run(&d, &args("migrate"), &mut &b""[..], &mut out)
        .await
        .unwrap();
    assert!(String::from_utf8_lossy(&out).contains("migrations: 3 applied"));

    let mut out = Vec::new();
    let line = "kelp forest at dawn\n";
    let cmd = args("staff create --admin --email root@sealbay.example --password-stdin");
    run(&d, &cmd, &mut line.as_bytes(), &mut out).await.unwrap();
    let text = String::from_utf8_lossy(&out);
    assert!(
        text.contains("created staff root@sealbay.example"),
        "{text}"
    );
    assert!(!text.contains("temporary password"));
    let app = h.boot(d).await;
    assert_eq!(
        app.browser()
            .login("root@sealbay.example", "kelp forest at dawn")
            .await
            .status,
        303
    );
}

#[test]
fn bad_command_lines_are_usage_errors() {
    for line in [
        "staff create",
        "staff create --email",
        "staff remove --email a@b",
        "frobnicate",
    ] {
        assert!(
            matches!(parse(&args(line)), Err(CliError::Usage(_))),
            "{line}"
        );
    }
    assert_eq!(parse(&[]).unwrap(), Command::Help);
    let short = args("staff create --email a@sealbay.example --password short");
    let Command::CreateStaff(a) = parse(&short).unwrap() else {
        panic!()
    };
    assert_eq!(format!("{:?}", a.password), "Given([redacted])");
}

#[tokio::test]
async fn short_passwords_are_refused() {
    let h = Harness::new();
    let d = h.builder().build().unwrap();
    let err = create_staff(&d, &admin("root@sealbay.example"), Some("short".into())).await;
    assert!(matches!(err, Err(CliError::Core(_))));
}
