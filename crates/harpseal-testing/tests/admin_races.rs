//! The last-manager rule under a race (#27 review): a Role change that was
//! fine when read must still be refused if, before it commits, another
//! request took `kernel.staff.manage` off the Role it relies on.

mod admin_support;
mod support;

use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex};

use admin_support::{CONFIG, PASSWORD, flash, hidden, staff_version};
use async_trait::async_trait;
use axum::body::Body;
use harpseal::{
    Actor, Db, DeploymentConfig, ExecResult, Locale, NewStaff, RequestCtx, Result, Rows, Statement,
};
use harpseal_testing::Harness;
use tower::ServiceExt;

type Pending = Pin<Box<dyn Future<Output = ()> + Send>>;

/// Runs `hook` once, just before the first batch whose audit row is
/// `action`: that request then commits after the hook's.
struct HookDb {
    inner: Arc<dyn Db>,
    action: &'static str,
    hook: Mutex<Option<Pending>>,
}

#[async_trait]
impl Db for HookDb {
    async fn query(&self, stmt: Statement) -> Result<Rows> {
        self.inner.query(stmt).await
    }
    async fn execute(&self, stmt: Statement) -> Result<ExecResult> {
        self.inner.execute(stmt).await
    }
    async fn batch(&self, stmts: Vec<Statement>) -> Result<Vec<ExecResult>> {
        let ours = stmts
            .first()
            .is_some_and(|s| s.params.iter().any(|p| p.as_text() == Some(self.action)));
        let hook = if ours {
            self.hook.lock().unwrap().take()
        } else {
            None
        };
        if let Some(h) = hook {
            h.await;
        }
        self.inner.batch(stmts).await
    }
}

#[tokio::test]
async fn a_role_losing_the_permission_meanwhile_refuses_the_roles_change() {
    let h = Harness::new();
    let db = Arc::new(HookDb {
        inner: h.db(),
        action: "staff.roles",
        hook: Mutex::new(None),
    });
    let mut services = h.services();
    services.db = db.clone();
    let config = DeploymentConfig::from_toml(CONFIG).unwrap();
    let d = h
        .builder()
        .config(config)
        .runtime(services)
        .build()
        .unwrap();
    let app = h.boot(d).await;
    let ctx = RequestCtx::default();
    let svc = h.services();
    let perms = ["kernel.staff.manage", "kernel.student.view"];
    harpseal::ensure_role(&svc, &Actor::System, "Chief", &perms, &ctx)
        .await
        .unwrap();
    let a_id = h
        .create_staff("a@sealbay.example", PASSWORD, &["Administrator"])
        .await;
    let mut chief = NewStaff::new("b@sealbay.example", "B", PASSWORD, Locale::th());
    chief.roles = vec!["Chief".into()];
    h.create_staff_with(chief).await;
    let a = app.browser();
    assert_eq!(a.login("a@sealbay.example", PASSWORD).await.status, 303);
    let b = app.browser();
    assert_eq!(b.login("b@sealbay.example", PASSWORD).await.status, 303);

    // B's request: take kernel.staff.manage off Chief (allowed on its own:
    // A still manages through Administrator).
    let rows = svc
        .db
        .query(Statement::new("SELECT id FROM role WHERE name = 'Chief'"))
        .await
        .unwrap();
    let chief_id = rows.get(0, "id").unwrap().as_text().unwrap().to_string();
    let edit = b.get(&format!("/admin/roles/{chief_id}")).await.body;
    let form = format!(
        "permission=kernel.student.view&version={}&_csrf={}",
        harpseal_testing::urlencode(&hidden(&edit, "version")),
        b.csrf().unwrap()
    );
    let req = http::Request::post(format!("/admin/roles/{chief_id}"))
        .header("content-type", "application/x-www-form-urlencoded")
        .header(
            "cookie",
            format!("hs_session={}", b.cookie("hs_session").unwrap()),
        )
        .body(Body::from(form))
        .unwrap();
    let router = app.router();
    *db.hook.lock().unwrap() = Some(Box::pin(async move {
        let resp = router.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), 303);
    }));

    // A: own Roles -> [Chief], read while Chief still grants the Permission;
    // B commits in between.
    let v = staff_version(&a, &a_id).await;
    let r = a
        .post(
            &format!("/admin/staff/{a_id}/roles"),
            &format!("role=Chief&version={v}"),
        )
        .await;
    assert_eq!(r.status, 303);
    assert!(db.hook.lock().unwrap().is_none(), "B ran first");
    assert_eq!(flash(&a), "err:kernel.admin.conflict");
    let managers = app
        .count(
            "SELECT count(*) FROM staff s JOIN staff_role sr ON sr.staff_id = s.id \
             JOIN role_permission rp ON rp.role_id = sr.role_id \
             WHERE rp.permission_id = 'kernel.staff.manage' AND s.status = 'active'",
        )
        .await;
    assert_eq!(managers, 1, "A still manages through Administrator");
    assert_eq!(
        app.count("SELECT count(*) FROM audit_log WHERE action = 'staff.roles'")
            .await,
        0
    );
    assert_eq!(
        app.count("SELECT count(*) FROM audit_log WHERE action = 'role.update'")
            .await,
        1
    );
}
