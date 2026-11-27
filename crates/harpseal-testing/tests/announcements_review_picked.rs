//! The hand-picked list on the review step and the live count while ids
//! are typed or picked: the review names each picked Student (id and
//! name, for Staff who may see Students) up to 100, then "และอีก N คน";
//! the count box counts again on typing and after the picker's swap.

mod announcements_support;

use announcements_support::*;
use harpseal_testing::{Browser, urlencode};

async fn world() -> World {
    let w = boot().await;
    roster(&w).await;
    w
}

/// Saves a draft to `list` and returns its review page.
async fn review(b: &Browser<'_>, list: &[String]) -> String {
    let extra = format!(
        "&scope=students&students={}&do=confirm",
        urlencode(&list.join("\n"))
    );
    let r = b
        .post(
            "/admin/announcements/new",
            &form("รายชื่อ", "เฉพาะบางคน", &extra),
        )
        .await;
    assert_eq!(r.status, 303, "{}", r.body);
    let to = r.header("location").unwrap().to_string();
    assert!(to.ends_with("/review"), "{to}");
    let page = b.get(&to).await;
    assert_eq!(page.status, 200, "{}", page.body);
    page.body
}

fn codes(list: &[&str]) -> Vec<String> {
    list.iter().map(|s| s.to_string()).collect()
}

#[tokio::test]
async fn the_review_names_each_picked_student() {
    let w = world().await;
    let b = signed_in(&w, "Officer").await;
    let page = review(&b, &codes(&["code-s3", "code-s1", "nope-0000"])).await;
    let at = page.find(r#"id="review-picked""#).expect("the picked list");
    let list = &page[at..at + page[at..].find("</ul>").unwrap()];
    // In list order, each id with its name; an id off the roster alone.
    let s3 = list.find("code-s3").unwrap();
    let s1 = list.find("code-s1").unwrap();
    assert!(s3 < s1, "{list}");
    assert!(
        list.contains("Name s3") && list.contains("Name s1"),
        "{list}"
    );
    assert!(list.contains("nope-0000"), "{list}");
    assert!(!page.contains("data-picked-more"), "{page}");
}

#[tokio::test]
async fn names_need_permission_to_see_students() {
    let w = world().await;
    let b = signed_in(&w, "Sender").await;
    let page = review(&b, &codes(&["code-s3", "code-s1"])).await;
    let at = page.find(r#"id="review-picked""#).expect("the picked list");
    let list = &page[at..at + page[at..].find("</ul>").unwrap()];
    assert!(
        list.contains("code-s3") && list.contains("code-s1"),
        "{list}"
    );
    assert!(!list.contains("Name s3"), "{list}");
}

#[tokio::test]
async fn a_long_list_shows_100_then_how_many_more() {
    let w = world().await;
    let b = signed_in(&w, "Officer").await;
    let mut list = codes(&["code-s1"]);
    list.extend((0..104).map(|i| format!("id-{i:04}")));
    let page = review(&b, &list).await;
    let at = page.find(r#"id="review-picked""#).expect("the picked list");
    let shown = &page[at..at + page[at..].find("</ul>").unwrap()];
    assert_eq!(shown.matches("<li>").count(), 100, "{shown}");
    assert!(shown.contains("id-0098") && !shown.contains("id-0099"));
    assert!(page.contains("และอีก 5 คน"), "{page}");
}

#[tokio::test]
async fn the_count_box_counts_while_ids_are_typed_and_after_a_pick() {
    let w = world().await;
    let b = signed_in(&w, "Officer").await;
    let page = b.get("/admin/announcements/new").await.body;
    let at = page.find(r#"id="audience-count""#).unwrap();
    let tag = &page[at..at + page[at..].find('>').unwrap()];
    // Typing in (or removing from) the id field counts again 300 ms
    // after the last key, not only when the field loses focus.
    assert!(
        tag.contains("input[target.id==&#39;students&#39;] delay:300ms from:closest form")
            || tag.contains("input[target.id=='students'] delay:300ms from:closest form"),
        "{tag}"
    );
    assert!(tag.contains("students-changed from:body"), "{tag}");
    // A newer count replaces one in flight (the box swaps itself, so a
    // queued one would be dropped), and the search box is no Audience.
    assert!(tag.contains(r#"hx-sync="this:replace""#), "{tag}");
    assert!(
        tag.contains("change[target.id!=") && tag.contains("pick-q"),
        "{tag}"
    );
    // The picker asks for the count after its swap: before it, the count
    // read the old field and lagged one id behind.
    let r = b
        .post(
            "/admin/announcements/pick",
            &format!("students={}&add=code-s4", urlencode("code-s1")),
        )
        .await;
    assert_eq!(r.status, 200, "{}", r.body);
    assert_eq!(r.header("hx-trigger-after-swap"), Some("students-changed"));
    assert_eq!(r.header("hx-trigger"), None);
}
