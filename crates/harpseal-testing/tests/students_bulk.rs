//! Bulk actions on the Students list (#145): a checkbox per row and the
//! choice of rows, then add or remove a manual Tag behind a review step
//! that freezes which Students change. Each action shows and works only
//! with its own Permission and writes one audit row per Student. Unlink
//! and announce are in `students_bulk_more.rs`.

mod bulk_support;
mod students_support;
mod support;

use bulk_support::{BULK, boot, confirm_of, hidden, many, review_then_confirm};
use harpseal_testing::a11y;
use students_support::{flash, run, signed_in};

#[tokio::test]
async fn the_list_shows_only_the_actions_a_permission_allows() {
    let (h, app) = boot().await;
    let (b, _) = signed_in(&h, &app, "Everything").await;
    let page = b.get("/admin/students").await.body;
    for action in ["tag_add", "tag_remove", "unlink", "announce"] {
        assert!(page.contains(&format!("value=\"{action}\"")), "{action}");
    }
    assert!(page.contains("name=\"student\" value=\"s1\""));
    assert!(page.contains("value=\"filter\""));
    // Without JS the count never changes: a neutral hint, not "nobody".
    assert!(page.contains("id=\"bulk-count\"") && page.contains("ติ๊กช่องหน้าชื่อ"));
    assert!(!page.contains("ยังไม่ได้เลือกใคร"));
    let problems = a11y::problems(&page);
    assert!(problems.is_empty(), "{problems:?}");

    let (t, _) = signed_in(&h, &app, "Tagger").await;
    let page = t.get("/admin/students").await.body;
    assert!(page.contains("value=\"tag_add\"") && page.contains("value=\"tag_remove\""));
    assert!(!page.contains("value=\"unlink\"") && !page.contains("value=\"announce\""));

    let (v, _) = signed_in(&h, &app, "Viewer").await;
    let page = v.get("/admin/students").await.body;
    assert!(!page.contains("name=\"student\""), "no checkboxes");
    assert!(
        !page.contains("id=\"bulk\"") && !page.contains("bulk-bar"),
        "no form"
    );
    // Crafted posts are refused, a confirm included.
    let r = v
        .post(BULK, "which=picked&student=s1&action=tag_add&tag=x")
        .await;
    assert_eq!(r.status, 403);
    let r = t.post(BULK, "which=picked&student=s1&action=unlink").await;
    assert_eq!(r.status, 403);
    let r = t
        .post(
            BULK,
            "which=picked&student=s1&action=unlink&confirm=yes&n=1&set=x",
        )
        .await;
    assert_eq!(r.status, 403);
    assert_eq!(app.count("SELECT count(*) FROM student_tag").await, 0);
    // An unknown action has its own message.
    assert_eq!(t.post(BULK, "which=page&action=delete").await.status, 303);
    assert_eq!(flash(&t), "err:kernel.students.bulk_unknown");
}

#[tokio::test]
async fn adding_a_tag_reviews_then_audits_each_student() {
    let (h, app) = boot().await;
    let (b, staff) = signed_in(&h, &app, "Tagger").await;
    // s1 already has it: only s3 changes. s2 has left and is tagged all
    // the same.
    let tag = "%E0%B8%97%E0%B8%B8%E0%B8%99";
    let pick = format!("which=picked&student=s1&student=s3&action=tag_add&tag={tag}");
    assert_eq!(
        b.post("/admin/students/s1/tags", "name=ทุน").await.status,
        303
    );
    let review = b.post(BULK, &pick).await;
    assert_eq!(review.status, 200, "{}", review.body);
    assert!(review.body.contains("ให้นักศึกษา 1 คน"), "{}", review.body);
    assert!(review.body.contains("อีก 1 คนมีป้ายนี้อยู่แล้ว"));
    // The review lists who changes, not who stays.
    assert!(review.body.contains("68010003") && !review.body.contains(">68010001"));
    assert!(a11y::problems(&review.body).is_empty());
    assert_eq!(
        app.count("SELECT count(*) FROM student_tag").await,
        1,
        "not yet"
    );

    let r = b
        .post(BULK, &format!("{pick}&{}", confirm_of(&review)))
        .await;
    assert_eq!(r.status, 303, "{}", r.body);
    assert_eq!(flash(&b), "ok:kernel.students.bulk_tag_added");
    let rows = app
        .query(
            "SELECT target_id, actor_id FROM audit_log WHERE action = 'student.tag_add' \
             ORDER BY rowid",
        )
        .await;
    assert_eq!(rows.len(), 2, "one per Student, the single add included");
    assert_eq!(rows.get(1, "target_id").unwrap().as_text(), Some("s3"));
    assert_eq!(
        rows.get(1, "actor_id").unwrap().as_text(),
        Some(staff.as_str())
    );

    // Remove from the whole filter, a left Student included.
    let left = format!("which=picked&student=s2&action=tag_add&tag={tag}");
    assert_eq!(review_then_confirm(&b, &left, "").await.status, 303);
    let all = format!("which=filter&q=&action=tag_remove&tag={tag}");
    let review = b.post(BULK, &all).await;
    assert!(review.body.contains("ออกจากนักศึกษา 3 คน"), "{}", review.body);
    assert_eq!(review_then_confirm(&b, &all, "").await.status, 303);
    assert_eq!(flash(&b), "ok:kernel.students.bulk_tag_removed");
    assert_eq!(app.count("SELECT count(*) FROM student_tag").await, 0);
    assert_eq!(
        app.count("SELECT count(*) FROM audit_log WHERE action = 'student.tag_remove'")
            .await,
        3
    );
}

#[tokio::test]
async fn a_changed_set_asks_again() {
    let (h, app) = boot().await;
    let (b, _) = signed_in(&h, &app, "Tagger").await;
    let all = "which=filter&q=&action=tag_add&tag=x";
    let r = b.post(BULK, &format!("{all}&confirm=yes&n=3")).await;
    assert_eq!(r.status, 200, "4 match, not 3");
    assert!(r.body.contains("รายชื่อเปลี่ยนไประหว่างนี้"));
    assert_eq!(app.count("SELECT count(*) FROM student_tag").await, 0);
    // Nobody picked, and a bad Tag name: back to the list.
    assert_eq!(
        b.post(BULK, "which=picked&action=tag_add&tag=x")
            .await
            .status,
        303
    );
    assert_eq!(flash(&b), "err:kernel.students.bulk_none");
    assert_eq!(
        b.post(BULK, "which=page&action=tag_add&tag=").await.status,
        303
    );
    assert_eq!(flash(&b), "err:kernel.students.tag_invalid");
}

#[tokio::test]
async fn a_page_that_shifts_never_swaps_the_students_reviewed() {
    let (h, app) = boot().await;
    many(&h, 60).await;
    let (b, _) = signed_in(&h, &app, "Tagger").await;
    let page = "which=page&page=1&q=&action=tag_add&tag=x";
    let review = b.post(BULK, page).await;
    assert!(review.body.contains("ให้นักศึกษา 50 คน"), "{}", review.body);
    // The page posts back its own rows as ticked rows.
    assert_eq!(hidden(&review.body, "which"), "picked");
    assert!(review.body.contains("name=\"student\" value=\"x45\""));
    assert!(!review.body.contains("name=\"student\" value=\"x46\""));
    // A roster import adds a code that sorts first: page 1 still has 50
    // rows, but x45 drops off and the new one comes in.
    run(
        &h,
        "INSERT INTO student (id, student_code, display_name, email, locale, status, \
         created_at, updated_at) VALUES ('n1', '67000001', 'Nam Mai', NULL, 'th', 'active', 1, 1)",
    )
    .await;
    let r = b
        .post(BULK, &format!("{page}&{}", confirm_of(&review)))
        .await;
    assert_eq!(r.status, 200, "same count, other Students: review again");
    assert!(r.body.contains("รายชื่อเปลี่ยนไประหว่างนี้"));
    assert_eq!(app.count("SELECT count(*) FROM student_tag").await, 0);
    // The review's own form (the rows it showed) still confirms them.
    let rows: String = (0..46)
        .map(|i| format!("&student=x{i}"))
        .chain(["s1", "s2", "s3", "s4"].map(|s| format!("&student={s}")))
        .collect();
    let shown = format!("which=picked&page=1&q=&action=tag_add&tag=x{rows}");
    let r = b
        .post(BULK, &format!("{shown}&{}", confirm_of(&review)))
        .await;
    assert_eq!(r.status, 303, "{}", r.body);
    assert_eq!(
        app.count("SELECT count(*) FROM student_tag WHERE student_id = 'n1'")
            .await,
        0,
        "the Student nobody saw is untouched"
    );
    assert_eq!(app.count("SELECT count(*) FROM student_tag").await, 50);
}

#[tokio::test]
async fn missing_data_filters_the_selection() {
    let (h, app) = boot().await;
    let (b, _) = signed_in(&h, &app, "Tagger").await;
    let list = b.get("/admin/students?missing=1").await.body;
    let shown = list.matches("class=\"name\" href=").count();
    let all = "which=filter&q=&missing=1&action=tag_add&tag=x";
    let review = b.post(BULK, all).await;
    assert_eq!(hidden(&review.body, "n"), shown.to_string());
    assert!(review.body.contains("68010003"), "s3 has no email");
    assert!(review.body.contains("name=\"missing\" value=\"1\""));
    let counted = b
        .post("/admin/students/bulk/count", "which=filter&q=&missing=1")
        .await
        .body;
    assert!(
        counted.contains(&format!("เลือกแล้ว {shown} คน")),
        "{counted}"
    );
}

#[tokio::test]
async fn the_count_and_buttons_sit_in_a_dock_after_the_last_row() {
    let (h, app) = boot().await;
    let (b, _) = signed_in(&h, &app, "Everything").await;
    let page = b.get("/admin/students").await.body;
    let at = |s: &str| page.find(s).unwrap_or_else(|| panic!("{s}"));
    // Inside the form, after the table: it never covers the last row.
    assert!(at("id=\"bulk\"") < at("</table>"));
    assert!(at("</table>") < at("class=\"bulk-dock\""));
    assert!(at("class=\"bulk-dock\"") < at("id=\"bulk-count\""));
    assert!(at("id=\"bulk-count\"") < at("value=\"tag_add\""));
    let form_end = at("id=\"bulk\"") + page[at("id=\"bulk\"")..].find("</form>").expect("end");
    assert!(at("value=\"announce\"") < form_end);
    // The checkbox's label fills its cell.
    assert!(page.contains("td class=\"pick\""));
    // No JS: the stylesheet sticks the dock once a box is ticked, and the
    // search stops sticking on phones.
    let css = harpseal::ui::CSS;
    assert!(css.contains("form.bulk:has(input[name=\"student\"]:checked"));
    assert!(css.contains(".search { position: static; }"));
    // One live region, refreshed by one HTMX post.
    assert_eq!(page.matches("id=\"bulk-count\"").count(), 1);
    assert_eq!(
        page.matches("hx-post=\"/admin/students/bulk/count\"")
            .count(),
        1
    );
    // The keyboard skips the rows to reach the buttons.
    assert!(at("href=\"#bulk-dock\"") < at("</table>"));
    assert!(page.contains("id=\"bulk-dock\""));
}
