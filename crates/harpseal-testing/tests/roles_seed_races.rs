//! `[roles]` seeding against a racing writer (#110 review): another
//! `migrate` seeds a pair (or a Role) and an Administrator removes it, all
//! between this `migrate`'s reads and its batch. The batch's guards see
//! `role_seed` as it is then, so nothing is granted again or created again,
//! and no audit row names a grant or a Role that never happened.

mod admin_support;
mod support;

use std::sync::{Arc, Mutex};

use admin_support::{CONFIG, audits, boot};
use async_trait::async_trait;
use harpseal::{Db, Deployment, DeploymentConfig, ExecResult, Result, Rows, Services, Statement};
use harpseal_testing::Harness;

/// The real database; before the seed's batch, runs `meddle` once.
struct Racing {
    inner: Arc<dyn Db>,
    meddle: Mutex<Option<Vec<Statement>>>,
}

#[async_trait]
impl Db for Racing {
    async fn query(&self, stmt: Statement) -> Result<Rows> {
        self.inner.query(stmt).await
    }
    async fn execute(&self, stmt: Statement) -> Result<ExecResult> {
        self.inner.execute(stmt).await
    }
    async fn batch(&self, stmts: Vec<Statement>) -> Result<Vec<ExecResult>> {
        let seeding = stmts.iter().any(|s| s.sql.contains("INTO role_seed"));
        let meddle = if seeding {
            self.meddle.lock().unwrap().take()
        } else {
            None
        };
        if let Some(writes) = meddle {
            self.inner.batch(writes).await?;
        }
        self.inner.batch(stmts).await
    }
}

/// The admin Deployment with TOML `toml`, on `h`'s database behind `Racing`.
fn racing(h: &Harness, toml: &str, meddle: &[&str]) -> Deployment {
    let db = Racing {
        inner: h.db(),
        meddle: Mutex::new(Some(
            meddle
                .iter()
                .map(|s| Statement::new(s.to_string()))
                .collect(),
        )),
    };
    let services = Services {
        db: Arc::new(db),
        ..h.services()
    };
    h.builder()
        .runtime(services)
        .config(DeploymentConfig::from_toml(toml).unwrap())
        .build()
        .unwrap()
}

async fn system_audits(h: &Harness, action: &str) -> i64 {
    let rows = h
        .db()
        .query(
            Statement::new(
                "SELECT count(*) AS n FROM audit_log WHERE action = ? AND actor_kind = 'system'",
            )
            .bind(action),
        )
        .await
        .unwrap();
    rows.get(0, "n").and_then(|v| v.as_i64()).unwrap()
}

#[tokio::test]
async fn a_pair_seeded_and_removed_meanwhile_is_not_granted_again() {
    let (h, _app) = boot().await;
    let toml = CONFIG.replace(
        r#"Reader = ["kernel.student.view"]"#,
        r#"Reader = ["kernel.student.view", "kernel.usage.view"]"#,
    );
    // The other migrate granted and recorded it; Admin then removed it.
    let d = racing(
        &h,
        &toml,
        &[
            "INSERT INTO role_seed (role_name, permission_id, seeded_at) \
           VALUES ('Reader', 'kernel.usage.view', 1)",
        ],
    );
    d.migrate().await.unwrap();
    let perms = harpseal::role_permissions(&h.services(), "Reader")
        .await
        .unwrap();
    assert_eq!(perms, ["kernel.student.view"]);
    assert_eq!(
        system_audits(&h, "role.update").await,
        0,
        "no grant, no audit"
    );

    // Nothing left unseen: a plain migrate does nothing either.
    racing(&h, &toml, &[]).migrate().await.unwrap();
    let perms = harpseal::role_permissions(&h.services(), "Reader")
        .await
        .unwrap();
    assert_eq!(perms, ["kernel.student.view"]);
}

#[tokio::test]
async fn a_role_created_and_deleted_meanwhile_is_not_created_again() {
    let (h, _app) = boot().await;
    let toml = CONFIG.replace(
        "[texts.th]",
        "Auditor = [\"kernel.audit.view\"]\n\n[texts.th]",
    );
    let created = audits(&h, "role.create").await;
    // The other migrate created and recorded Auditor; Admin deleted it.
    let d = racing(
        &h,
        &toml,
        &[
            "INSERT INTO role_seed (role_name, permission_id, seeded_at) VALUES \
             ('Auditor', '', 1), ('Auditor', 'kernel.audit.view', 1)",
        ],
    );
    d.migrate().await.unwrap();
    let rows = h
        .db()
        .query(Statement::new("SELECT 1 FROM role WHERE name = 'Auditor'"))
        .await
        .unwrap();
    assert!(rows.is_empty(), "not re-created");
    assert_eq!(
        audits(&h, "role.create").await,
        created,
        "no role.create for a Role never inserted"
    );
}
