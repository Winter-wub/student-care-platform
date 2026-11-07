//! Helpers for the edit notice tests (#77).

use harpseal::Message;

use super::*;

/// The notice Card's title (Thai).
pub const NOTICE: &str = "ข่าวนี้มีการแก้ไข กดดูฉบับล่าสุดได้เลย";
/// The same in English.
pub const NOTICE_EN: &str = "This announcement was edited. Open it for the latest version.";

/// Notice Cards `student` got on the LINE-like Channel.
pub fn notices(w: &World, student: &str) -> Vec<String> {
    w.line
        .sent_to(&fake_account("fake", student))
        .into_iter()
        .filter_map(|m| match m {
            Message::Card(c) if c.title == NOTICE || c.title == NOTICE_EN => Some(c.body),
            _ => None,
        })
        .collect()
}

pub async fn metered(w: &World, channel: &str) -> i64 {
    count(
        w,
        &format!("SELECT coalesce(sum(metered), 0) FROM usage_daily WHERE channel = '{channel}'"),
    )
    .await
}

/// Sends a new announcement to everyone and lets it go out.
pub async fn sent(w: &World, b: &harpseal_testing::Browser<'_>) -> String {
    let id = create(b, &form("เดิม", "ข้อความเดิม", "&do=confirm")).await;
    drain(w).await;
    w.line.take_sent();
    w.tg.take_sent();
    id
}

/// Saves the edit form of `id` with `title`, ticking "notify" if `notify`.
pub async fn edit(
    w: &World,
    b: &harpseal_testing::Browser<'_>,
    id: &str,
    title: &str,
    notify: bool,
) {
    let path = format!("/admin/announcements/{id}/edit");
    assert_eq!(b.get(&path).await.status, 200);
    let v = version(w, id).await;
    let extra = format!("&version={v}{}", if notify { "&notify=1" } else { "" });
    let r = b.post(&path, &form(title, "ข้อความใหม่", &extra)).await;
    assert_eq!(r.status, 303, "{}", r.body);
}

/// Posts the edit form of `id` as is (`form` urlencoded, version added);
/// returns the response.
pub async fn post_edit(
    w: &World,
    b: &harpseal_testing::Browser<'_>,
    id: &str,
    form: &str,
) -> harpseal_testing::TestResponse {
    let path = format!("/admin/announcements/{id}/edit");
    assert_eq!(b.get(&path).await.status, 200);
    let v = version(w, id).await;
    b.post(&path, &format!("{form}&version={v}")).await
}
