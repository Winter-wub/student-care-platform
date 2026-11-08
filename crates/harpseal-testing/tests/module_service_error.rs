//! `ui::service_error` from a Module page (#106): a Core service's
//! `Conflict` is a 409 page asking to reload, any other failure a 500 page
//! that never shows the error text; `Forbidden` is covered by the example's
//! `reminders_announce` test.

use axum::extract::State;
use axum::response::Response;
use axum::routing::get;
use harpseal::{AppState, CurrentStaff, Error, Module, ModuleRegistrar, ui};
use harpseal_testing::Harness;

const PASSWORD: &str = "kelp forest at dawn";

struct Failing;

async fn conflict(State(s): State<AppState>, staff: CurrentStaff) -> Response {
    ui::service_error(&s, &staff, &Error::Conflict("announcement 42 v3".into()))
}

async fn db(State(s): State<AppState>, staff: CurrentStaff) -> Response {
    let e = Error::Db("no such column: secret_ledger_col".into());
    ui::service_error(&s, &staff, &e)
}

async fn partial(State(s): State<AppState>, staff: CurrentStaff) -> Response {
    let e = Error::partial_send(1, 2, Error::Conflict("inner".into()));
    ui::service_error(&s, &staff, &e)
}

impl Module for Failing {
    fn name(&self) -> &'static str {
        "failing"
    }
    fn register(&self, r: &mut ModuleRegistrar) {
        r.back_office("kernel.student.view", "/failing/conflict", get(conflict))
            .back_office("kernel.student.view", "/failing/db", get(db))
            .back_office("kernel.student.view", "/failing/partial", get(partial));
    }
}

#[tokio::test]
async fn a_conflict_asks_to_reload_and_other_errors_hide_their_text() {
    let h = Harness::new();
    let app = h.boot(h.builder().module(Failing).build().unwrap()).await;
    h.create_staff("viewer@sealbay.example", PASSWORD, &["Viewer"])
        .await;
    let b = app.browser();
    b.login("viewer@sealbay.example", PASSWORD).await;

    let r = b.get("/admin/failing/conflict").await;
    assert_eq!(r.status, 409);
    assert!(r.body.contains("กรุณาโหลดหน้าใหม่"), "{}", r.body);
    assert!(!r.body.contains("announcement 42"), "no error text");
    // Inside the back-office layout (the sign-out form).
    assert!(r.body.contains("action=\"/admin/logout\""));

    // The root of a partial send decides.
    assert_eq!(b.get("/admin/failing/partial").await.status, 409);

    let r = b.get("/admin/failing/db").await;
    assert_eq!(r.status, 500);
    assert!(r.body.contains("ระบบขัดข้อง"), "{}", r.body);
    assert!(!r.body.contains("secret_ledger_col"), "no error text");
    assert!(!r.body.contains("no such column"));
}
