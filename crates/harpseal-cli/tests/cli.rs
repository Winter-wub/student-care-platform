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
    assert!(String::from_utf8_lossy(&out).contains("migrations: 19 applied"));

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

#[tokio::test]
async fn an_administrator_role_from_the_toml_wins_with_a_warning() {
    // TEST_CONFIG seeds Administrator with three Permissions only.
    let h = Harness::new();
    let d = h.builder().build().unwrap();
    let mut out = Vec::new();
    let cmd = args("staff create --admin --email root@sealbay.example");
    run(&d, &cmd, &mut &b""[..], &mut out).await.unwrap();
    let text = String::from_utf8_lossy(&out);
    assert!(text.contains("warning: role Administrator"), "{text}");
    assert!(text.contains("kernel.audit.view"), "{text}");
    assert!(
        !text.contains("kernel.student.view,"),
        "held ones are not listed: {text}"
    );
}

#[tokio::test]
async fn staff_rename_sets_the_display_name_and_is_audited() {
    let h = Harness::new();
    let d = h.builder().build().unwrap();
    let mut out = Vec::new();
    let create = args("staff create --email pim@sealbay.example --password-stdin");
    run(&d, &create, &mut &b"a long enough password\n"[..], &mut out)
        .await
        .unwrap();
    let mut rename: Vec<String> = args("staff rename --email Pim@SealBay.example --name");
    rename.push("  Pim Officer  ".into());
    let mut out = Vec::new();
    run(&d, &rename, &mut &b""[..], &mut out).await.unwrap();
    assert_eq!(
        String::from_utf8(out).unwrap(),
        "renamed staff pim@sealbay.example to Pim Officer\n"
    );
    let app = h.boot(d).await;
    assert_eq!(
        app.count(
            "SELECT count(*) FROM staff WHERE display_name = 'Pim Officer' \
             AND email = 'pim@sealbay.example'"
        )
        .await,
        1
    );
    assert_eq!(
        app.count(
            "SELECT count(*) FROM audit_log WHERE action = 'staff.rename' \
             AND actor_kind = 'system' AND before = '{\"display_name\":\"pim\"}' \
             AND after = '{\"display_name\":\"Pim Officer\"}'"
        )
        .await,
        1
    );
}

#[tokio::test]
async fn staff_rename_refuses_unknown_emails_and_bad_names() {
    let h = Harness::new();
    let d = h.builder().build().unwrap();
    let mut out = Vec::new();
    let create = args("staff create --email pim@sealbay.example --password-stdin");
    run(&d, &create, &mut &b"a long enough password\n"[..], &mut out)
        .await
        .unwrap();
    let r = harpseal_cli::rename_staff(&d, "nobody@sealbay.example", "Nobody").await;
    assert!(matches!(r, Err(CliError::Refused(_))), "{r:?}");
    let long = "x".repeat(101);
    for bad in ["  ", "Pim\u{1b}[31m", long.as_str()] {
        let r = harpseal_cli::rename_staff(&d, "pim@sealbay.example", bad).await;
        assert!(matches!(r, Err(CliError::Refused(_))), "{bad:?}: {r:?}");
    }
    for bad in [
        "staff rename --email pim@sealbay.example",
        "staff rename --name Pim",
        "staff rename --email pim@sealbay.example --name",
        "staff rename --email pim@sealbay.example --name Pim --role Officer",
    ] {
        assert!(
            matches!(parse(&args(bad)), Err(CliError::Usage(_))),
            "{bad}"
        );
    }
    let app = h.boot(d).await;
    assert_eq!(
        app.count("SELECT count(*) FROM audit_log WHERE action = 'staff.rename'")
            .await,
        0
    );
}
