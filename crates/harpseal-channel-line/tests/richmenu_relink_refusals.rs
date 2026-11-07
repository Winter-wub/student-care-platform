//! The Menu re-link (#66) on recorded LINE HTTP when LINE refuses: an id
//! that is no user id, or one a bulk call names, is skipped; a 400 that
//! names no user falls back to per-user links within a budget per run; a
//! rich menu refused on every link stops the run without skipping anyone.

mod common;

use common::relink::*;
use harpseal::Statement;

#[tokio::test]
async fn a_refused_or_malformed_user_id_is_skipped_and_the_rest_relinked() {
    let s = setup(4).await;
    let sql = "UPDATE channel_account SET external_id = 'not-a-line-id' WHERE id = 'ca00001'";
    s.h.db().execute(Statement::new(sql)).await.unwrap();
    s.api.refuse_user(&user(2));
    s.save(EDITED).await;
    s.drain().await;
    let rev1 = s.api.linked(&user(0)).unwrap();
    assert!(is_rev(&s.shown(0), 1));
    assert_eq!(s.api.linked(&user(3)), Some(rev1));
    let bulk = bulk_calls(&s.api.requests());
    let sizes: Vec<usize> = bulk.iter().map(|(_, n)| *n).collect();
    assert_eq!(
        sizes,
        [3, 2],
        "the malformed id never sent; the refused one dropped"
    );
    let live =
        s.h.db()
            .query(Statement::new(
                "SELECT count(*) AS n FROM menu_revision WHERE revision = 1 AND state = 'live'",
            ))
            .await
            .unwrap();
    assert_eq!(live.get(0, "n").and_then(|v| v.as_i64()), Some(1));
}

#[tokio::test]
async fn a_rich_menu_refused_on_every_link_stops_the_run_and_skips_nobody() {
    let s = setup(60).await;
    let rev0 = s.api.linked(&user(0)).unwrap();
    s.api.take();
    // As for a rich menu without an image: bulk and per-user links 400.
    s.api
        .refuse_links(Some(("Couldn't link the rich menu: no image set", true)));
    s.save(EDITED).await;
    let r = s.tick().await;
    assert_eq!(r.retried, 1, "{r:?}");
    assert_eq!(
        per_user_links(&s.api.take()),
        1,
        "stops at the first menu-level 400"
    );
    for i in [0, 59] {
        assert_eq!(s.api.linked(&user(i)), Some(rev0.clone()), "account {i}");
    }
    let rows =
        s.h.db()
            .query(Statement::new(
                "SELECT (SELECT state FROM menu_revision WHERE revision = 1) AS st, \
             (SELECT count(*) FROM link WHERE status <> 'active') AS bad",
            ))
            .await
            .unwrap();
    assert_eq!(rows.get(0, "st").and_then(|v| v.as_text()), Some("linking"));
    assert_eq!(rows.get(0, "bad").and_then(|v| v.as_i64()), Some(0));

    // Fixed on LINE's side: the retry re-links everyone.
    s.api.refuse_links(None);
    s.minutes(1);
    s.drain().await;
    assert!(is_rev(&s.shown(59), 1));
}

#[tokio::test]
async fn the_per_user_fallback_stays_within_its_budget_per_run() {
    let s = setup(100).await;
    s.api.take();
    // Bulk refused for a reason naming no user; per-user links work.
    s.api
        .refuse_links(Some(("The request body has 1 error(s)", false)));
    s.save(EDITED).await;
    let mut per_run = Vec::new();
    for _ in 0..5 {
        s.tick().await;
        per_run.push(per_user_links(&s.api.take()));
        s.minutes(1);
    }
    assert_eq!(per_run, [40, 40, 20, 0, 0], "{per_run:?}");
    for i in [0, 40, 99] {
        assert!(is_rev(&s.shown(i), 1), "account {i}");
    }
}
