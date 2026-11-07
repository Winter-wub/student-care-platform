//! The Menu re-link after a label save (#66) on recorded LINE HTTP: a
//! Deployment with the LINE Channel and linked accounts; a Staff member
//! saves a Menu label; the Kernel Job bulk-links every account to the new
//! revision (calls of at most 500), then retires the previous salted
//! revision through batch control, deleting it only once the progress
//! endpoint says `succeeded`. Revision 0 (shared by every Deployment on
//! the token with the default labels) and another Deployment's revisions
//! are never moved or deleted; batches never overlap; a failed bulk link
//! retries without deleting anything; a refused user id is skipped.

mod common;

use common::LineApi;
use common::relink::*;

#[tokio::test]
async fn a_label_save_relinks_1200_accounts_in_3_bulk_calls_then_retires_the_previous_revision() {
    let s = setup(1200).await;
    let rev0 = s.api.linked(&user(0)).unwrap();
    s.save(EDITED).await;
    s.tick().await;
    let bulk = bulk_calls(&s.api.take());
    let sizes: Vec<usize> = bulk.iter().map(|(_, n)| *n).collect();
    assert_eq!(sizes, [500, 500, 200], "3 bulk calls");
    let rev1 = bulk[0].0.clone();
    assert!(bulk.iter().all(|(id, _)| *id == rev1));
    let created = s.api.created(&rev1).unwrap();
    let name = created["name"].as_str().unwrap().to_string();
    assert!(is_rev(&name, 1), "{name}");
    assert!(created["areas"].to_string().contains(EDITED));
    for i in [0, 499, 500, 1199] {
        assert_eq!(s.api.linked(&user(i)), Some(rev1.clone()), "account {i}");
    }
    // Revision 0 has no salt: it may be another Deployment's too.
    assert!(s.api.batches().is_empty() && s.deletes().is_empty());

    s.save(FIXED).await;
    s.api.queue_phase("ongoing");
    s.minutes(1);
    s.tick().await;
    let rev2 = bulk_calls(&s.api.requests())[0].0.clone();
    assert!(is_rev(
        s.api.created(&rev2).unwrap()["name"].as_str().unwrap(),
        2
    ));
    assert_eq!(s.api.batches().len(), 1);
    assert_eq!(s.api.batches()[0].1, [(rev1.clone(), rev2.clone())]);
    // The first poll waits 5 minutes, and "ongoing" deletes nothing.
    s.minutes(4);
    s.tick().await;
    assert!(!s.api.requests().iter().any(|r| r.uri.starts_with(PROGRESS)));
    s.minutes(1);
    s.tick().await;
    assert!(s.deletes().is_empty(), "still ongoing");
    s.minutes(10);
    s.tick().await;
    assert_eq!(s.deletes(), [rev1], "only the previous salted revision");
    assert!(s.api.live_names().len() == 2, "revision 0 and 2 stay");
    assert!(s.api.id_of(&s.shown(0)).is_some());
    assert_eq!(s.api.linked(&user(0)), Some(rev2));
    assert_ne!(s.api.linked(&user(0)), Some(rev0));
}

#[tokio::test]
async fn a_failed_bulk_link_retries_and_never_deletes_early() {
    let s = setup(1200).await;
    s.save(EDITED).await;
    s.drain().await;
    let rev1 = s.api.linked(&user(1199)).unwrap();
    s.api.take();
    s.save(FIXED).await;
    // list, create, upload, bulk 1: fine; bulk 2: LINE fails.
    for _ in 0..4 {
        s.api.http().respond_with(200, "{}");
    }
    s.api
        .http()
        .respond_with(500, r#"{"message":"An error occurred in the backend"}"#);
    let r = s.tick().await;
    assert_eq!(r.retried, 1, "{r:?}");
    let reqs = s.api.take();
    assert_eq!(bulk_calls(&reqs).len(), 2);
    assert!(s.api.batches().is_empty(), "no retire yet");
    assert!(s.deletes().is_empty());
    assert_eq!(
        s.api.linked(&user(1199)),
        Some(rev1.clone()),
        "not re-linked yet"
    );

    s.minutes(1);
    s.tick().await;
    let sizes: Vec<usize> = bulk_calls(&s.api.requests())
        .iter()
        .map(|(_, n)| *n)
        .collect();
    assert_eq!(sizes, [500, 500, 200], "the retry re-links every account");
    assert_eq!(s.api.batches().len(), 1);
    s.drain().await;
    assert_eq!(s.deletes(), [rev1]);
}

#[tokio::test]
async fn two_deployments_on_one_token_never_touch_each_others_menus() {
    let api = LineApi::default();
    let prod = deployment(&api, 0x5EA1, 0, 3).await;
    let staging = deployment(&api, 0xBA7, 100, 102).await;
    let rev0 = api.linked(&user(0)).unwrap();
    assert_eq!(
        api.linked(&user(100)),
        Some(rev0.clone()),
        "one shared rich menu"
    );
    // Staging saves the very label prod will save.
    staging.save(EDITED).await;
    staging.drain().await;
    let staging_r1 = api.linked(&user(100)).unwrap();

    prod.save(EDITED).await;
    prod.drain().await;
    let prod_r1 = api.linked(&user(0)).unwrap();
    assert_ne!(
        prod_r1, staging_r1,
        "same labels, other salt: other rich menus"
    );
    prod.save(FIXED).await;
    prod.drain().await;

    let touched: Vec<String> = api
        .batches()
        .iter()
        .flat_map(|(_, ops)| ops.iter().map(|(from, _)| from.clone()))
        .chain(prod.deletes())
        .collect();
    assert_eq!(touched, [prod_r1.clone(), prod_r1], "batch from + DELETE");
    assert!(api.id_of(&staging.shown(100)).is_some());
    assert_eq!(api.linked(&user(100)), Some(staging_r1.clone()));
    assert_eq!(api.linked(&user(101)), Some(staging_r1));
    let live = api.live_names();
    assert!(
        live.iter().any(|n| api.id_of(n) == Some(rev0.clone())),
        "revision 0 stays: {live:?}"
    );
}

#[tokio::test]
async fn a_second_batch_waits_for_the_first_to_finish() {
    let s = setup(3).await;
    s.save(EDITED).await;
    s.drain().await;
    let rev1 = s.api.linked(&user(0)).unwrap();
    // Save 2: batch A (rev1 -> rev2) stays ongoing for a while.
    s.save(FIXED).await;
    for _ in 0..3 {
        s.api.queue_phase("ongoing");
    }
    s.minutes(1);
    s.tick().await;
    let rev2 = s.api.linked(&user(0)).unwrap();
    assert_eq!(s.api.batches().len(), 1);
    // Save 3 (a typo fix) while A is ongoing: run B re-links to rev3 and
    // takes A over; it sends nothing new while A is ongoing.
    s.save("ยกเลิกการเชื่อมต่อบัญชี").await;
    s.minutes(1);
    s.tick().await;
    let rev3 = s.api.linked(&user(0)).unwrap();
    assert_ne!(rev3, rev2);
    assert_eq!(s.api.batches().len(), 1, "A ongoing: B sends no batch");
    for _ in 0..2 {
        s.minutes(61);
        s.tick().await;
        assert_eq!(s.api.batches().len(), 1, "A still ongoing");
        assert!(s.deletes().is_empty());
    }
    s.drain().await;
    let batches = s.api.batches();
    assert_eq!(batches.len(), 2, "{batches:?}");
    assert_eq!(batches[0].1, [(rev1.clone(), rev2.clone())]);
    assert_eq!(batches[1].1, [(rev2.clone(), rev3.clone())]);
    assert_eq!(s.deletes(), [rev1, rev2]);
    // The second batch went out only after A's progress said succeeded.
    let reqs = s.api.requests();
    let polls = reqs.iter().filter(|r| r.uri.starts_with(PROGRESS)).count();
    assert_eq!(polls, 5, "4 for A (3 ongoing, then succeeded), 1 for B");
}
