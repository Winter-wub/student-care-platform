//! Typed Audience filters (#156): a small whole-number attribute (a year)
//! is checkboxes over `attr.year`; a numeric one (a GPA) is a range,
//! `attr_min.gpa` / `attr_max.gpa`, stored as `ranges` beside the
//! attributes. Bounds are inclusive, a missing or non-numeric value never
//! matches, a bad range is refused in Thai, and rows stored before #156
//! (exact values) load unchanged and keep their values.

mod announcements_support;

use announcements_support::*;
use harpseal_testing::{Browser, urlencode};

const LIST: &str = "/admin/announcements/audiences";
const COUNT: &str = "/admin/announcements/audience";

/// [`CONFIG`] with the year between 1 and 6 and an optional GPA.
fn config() -> String {
    CONFIG.replace(
        "label = { th = \"ชั้นปี\", en = \"Year\" }\n",
        "label = { th = \"ชั้นปี\", en = \"Year\" }\nmin = 1\nmax = 6\n\n\
         [roster.attributes.gpa]\ncolumn = \"เกรดเฉลี่ย\"\ntype = \"number\"\noptional = true\n\
         label = { th = \"เกรดเฉลี่ย\", en = \"GPA\" }\n",
    )
}

/// Five linked Students in year 2: GPAs 3.0, 3.5 and 4.0, none, and a
/// text that is not a number.
async fn world() -> World {
    let w = boot_config(&config(), 500).await;
    for (id, gpa) in [
        ("g1", r#","gpa":3.0"#),
        ("g2", r#","gpa":3.5"#),
        ("g3", r#","gpa":4"#),
        ("g4", ""),
        ("g5", r#","gpa":"สาม""#),
    ] {
        let attrs = format!(r#"{{"programme":"MSC","year":2{gpa}}}"#);
        student(&w.h, id, "th", "active", &attrs).await;
        link(&w.h, id, "fake", "active").await;
    }
    w
}

async fn opened<'a>(w: &'a World) -> Browser<'a> {
    let b = signed_in(w, "Officer").await;
    assert_eq!(b.get("/admin/announcements/new").await.status, 200);
    b
}

/// The number in the count box's "นักศึกษา N".
fn counted(body: &str) -> String {
    let at = body.find("</strong>").expect("a count");
    body[..at].rsplit(' ').next().unwrap().to_string()
}

#[tokio::test]
async fn bounds_are_inclusive_and_a_missing_or_text_value_never_matches() {
    let w = world().await;
    let b = opened(&w).await;
    for (rules, n, who) in [
        ("attr_min.gpa=3.0&attr_max.gpa=3.5", "2", "g1, g2"),
        ("attr_min.gpa=3.5", "2", "g2, g3"),
        ("attr_max.gpa=3", "1", "g1 only: never g4 or g5"),
        ("attr_min.gpa=0", "3", "every number, never g4 or g5"),
        ("attr_min.gpa=3.5&attr_max.gpa=3.5", "1", "g2"),
        // AND-ed with the other filters.
        ("attr_min.gpa=3&attr.year=3", "0", "nobody in year 3"),
        // Neither bound: no range, the year alone.
        ("attr_min.gpa=&attr_max.gpa=+&attr.year=2", "5", "all five"),
    ] {
        let c = b.post(COUNT, &format!("scope=groups&{rules}")).await;
        assert_eq!(c.status, 200);
        assert_eq!(counted(&c.body), n, "{who}: {}", c.body);
    }
}

#[tokio::test]
async fn a_bad_range_is_refused_in_thai_and_nothing_is_stored() {
    let w = world().await;
    let b = opened(&w).await;
    for (rules, said) in [
        (
            "attr_min.gpa=3.5&attr_max.gpa=3",
            "ช่อง &quot;ตั้งแต่&quot; มีค่ามากกว่าช่อง &quot;ถึง&quot;",
        ),
        ("attr_min.gpa=abc", "ช่วงค่าต้องเป็นตัวเลข"),
        ("attr_max.gpa=inf", "ช่วงค่าต้องเป็นตัวเลข"),
    ] {
        let audience = format!("&scope=groups&{rules}");
        let r = b
            .post(
                "/admin/announcements/new",
                &form("ช่วง", "ข้อความ", &audience),
            )
            .await;
        assert_eq!(r.status, 422, "{}", r.body);
        assert!(r.body.contains(said), "{rules}: {}", r.body);
        // Kept as typed, so Staff correct it rather than retype it.
        let typed = rules.split('&').next().unwrap().split('=').nth(1).unwrap();
        assert!(
            r.body.contains(&format!(r#"value="{typed}""#)),
            "{}",
            r.body
        );
        // The count says the same instead of counting.
        let c = b.post(COUNT, &audience[1..]).await;
        assert!(c.body.contains(said), "{rules}: {}", c.body);
        assert!(!c.body.contains("</strong>"), "no count: {}", c.body);
        // And a saved Audience is refused too.
        let s = b
            .post(LIST, &format!("{}&saved_name=bad", &audience[1..]))
            .await;
        assert!(s.body.contains(said), "{rules}: {}", s.body);
    }
    assert_eq!(count(&w, "SELECT COUNT(*) FROM announcement").await, 0);
    assert_eq!(count(&w, "SELECT COUNT(*) FROM saved_audience").await, 0);
}

#[tokio::test]
async fn a_range_round_trips_through_a_draft_and_a_saved_audience() {
    let w = world().await;
    let b = opened(&w).await;
    let rules = "scope=groups&attr.year=2&attr_min.gpa=3.0&attr_max.gpa=3.5";
    // Untouched range boxes are no rule: never stored.
    let empty = "&scope=groups&attr.year=2&attr_min.gpa=&attr_max.gpa=";
    create(&b, &form("ไม่มีช่วง", "ข้อความ", empty)).await;
    let id = create(&b, &form("ช่วง", "ข้อความ", &format!("&{rules}"))).await;
    let stored = texts(&w, "SELECT audience FROM announcement ORDER BY rowid").await;
    assert_eq!(
        stored,
        [
            r#"{"attributes":{"year":["2"]},"tags":[],"include_unreachable":false}"#,
            r#"{"attributes":{"year":["2"]},"ranges":{"gpa":{"min":"3.0","max":"3.5"}},"tags":[],"include_unreachable":false}"#
        ]
    );
    let audit = texts(
        &w,
        "SELECT after ->> '$.audience.ranges.gpa.min' FROM audit_log \
         WHERE action = 'announcement.create' AND after ->> '$.audience.ranges' IS NOT NULL",
    )
    .await;
    assert_eq!(audit, ["3.0"]);
    // The detail page and the list say it in words.
    let detail = b.get(&format!("/admin/announcements/{id}")).await;
    assert!(
        detail.body.contains("เกรดเฉลี่ย: ตั้งแต่ 3.0 ถึง 3.5"),
        "{}",
        detail.body
    );
    let list = b.get("/admin/announcements").await;
    assert!(list.body.contains("ตั้งแต่ 3.0 ถึง 3.5"), "{}", list.body);
    // The edit form holds it again: the year checked, the bounds typed.
    let edit = b.get(&format!("/admin/announcements/{id}/edit")).await;
    assert_eq!(edit.status, 200, "{}", edit.body);
    for want in [
        r#"name="attr.year" value="2" checked"#,
        r#"name="attr_min.gpa" value="3.0""#,
        r#"name="attr_max.gpa" value="3.5""#,
    ] {
        assert!(edit.body.contains(want), "{want}: {}", edit.body);
    }
    // The review step names the range.
    let r = b
        .post(
            "/admin/announcements/new",
            &form("ทบทวน", "ข้อความ", &format!("&{rules}&do=confirm")),
        )
        .await;
    assert_eq!(r.status, 303, "{}", r.body);
    let review = b.get(r.header("location").unwrap()).await;
    assert!(review.body.contains("ตั้งแต่ 3.0 ถึง 3.5"), "{}", review.body);

    // Saved: JSON v1 with the range beside the attributes.
    let s = b
        .post(LIST, &format!("{rules}&saved_name={}", urlencode("เกรดดี")))
        .await;
    assert_eq!(s.status, 200, "{}", s.body);
    let saved = texts(&w, "SELECT audience FROM saved_audience").await;
    assert_eq!(
        saved,
        [
            r#"{"attributes":{"year":["2"]},"include_unreachable":false,"ranges":{"gpa":{"max":"3.5","min":"3.0"}},"tags":[],"v":1}"#
        ]
    );
    let page = b.get(LIST).await;
    assert!(
        page.body.contains("เกรดเฉลี่ย: ตั้งแต่ 3.0 ถึง 3.5"),
        "{}",
        page.body
    );
    let sid = texts(&w, "SELECT id FROM saved_audience").await.remove(0);
    let used = b
        .post(&format!("{LIST}/use"), &format!("saved_audience={sid}"))
        .await;
    for want in [
        r#"name="attr.year" value="2" checked"#,
        r#"name="attr_min.gpa" value="3.0""#,
        r#"name="attr_max.gpa" value="3.5""#,
    ] {
        assert!(used.body.contains(want), "{want}: {}", used.body);
    }
}

/// Inserts a saved Audience row as stored before #156.
async fn insert(w: &World, id: &str, audience: &str) {
    run(
        &w.h,
        &format!(
            "INSERT INTO saved_audience (id, name, scope, audience, created_at, updated_at) \
             VALUES ('{id}', 'name-{id}', 'groups', '{audience}', 1, 1)"
        ),
    )
    .await;
}

#[tokio::test]
async fn an_old_saved_audience_keeps_its_exact_values() {
    let w = world().await;
    // Exact GPA values and a year outside 1-6, saved before #156.
    insert(
        &w,
        "sa-old",
        r#"{"v":1,"attributes":{"gpa":["3.5"],"year":["2","7"]},"tags":[]}"#,
    )
    .await;
    let b = opened(&w).await;
    let used = b
        .post(&format!("{LIST}/use"), "saved_audience=sa-old")
        .await;
    assert_eq!(used.status, 200, "{}", used.body);
    for want in [
        // The year: checkboxes 1-6, the old 7 one more, checked.
        r#"name="attr.year" value="1">"#,
        r#"name="attr.year" value="2" checked"#,
        r#"name="attr.year" value="7" checked"#,
        // The GPA: empty range boxes, the exact value kept under them.
        r#"name="attr_min.gpa" value="""#,
        r#"name="attr.gpa" value="3.5""#,
        "ค่าที่ระบุไว้เดิม",
    ] {
        assert!(used.body.contains(want), "{want}: {}", used.body);
    }
    // Posted back as shown, it counts as before: g2 only.
    let c = b
        .post(
            COUNT,
            "scope=groups&attr.year=2&attr.year=7&attr.gpa=3.5&attr_min.gpa=&attr_max.gpa=",
        )
        .await;
    assert_eq!(counted(&c.body), "1", "{}", c.body);
}

#[tokio::test]
async fn the_form_with_typed_filters_passes_the_accessibility_checks() {
    let w = world().await;
    insert(
        &w,
        "sa-old",
        r#"{"v":1,"attributes":{"gpa":["3.5"]},"ranges":{"gpa":{"min":"3"}},"tags":[]}"#,
    )
    .await;
    let b = opened(&w).await;
    for path in [
        "/admin/announcements/new",
        "/admin/announcements/new?audience=sa-old",
    ] {
        let page = b.get(path).await;
        assert_eq!(page.status, 200, "{path}");
        assert_eq!(
            harpseal_testing::a11y::problems(&page.body),
            Vec::<String>::new(),
            "{path}"
        );
        assert_eq!(page.body.matches("<h1").count(), 1, "{path}");
    }
}
