//! Helpers for the confirmation in proportion to reach (#120): the
//! Audience choice a form implies, the review step's fields, and confirms
//! and resends done as Staff do them.

use harpseal_testing::{Browser, TestResponse};

/// `form` with the Audience choice (#120) its filters imply, unless it
/// names one: `groups` with an attribute or Tag, `students` with a list,
/// else `everyone`. Groups and a list together stay without a choice, as
/// an older page posts them (the Audience ANDs them; a choice would keep
/// only its own filters).
pub fn with_scope(form: &str) -> String {
    if form.split('&').any(|p| p.starts_with("scope=")) {
        return form.to_string();
    }
    let filled = |p: &str| p.split_once('=').is_some_and(|(_, v)| !v.is_empty());
    let has = |k: &str| form.split('&').any(|p| p.starts_with(k) && filled(p));
    let groups = has("attr.") || has("tags=");
    if groups && has("students=") {
        return form.to_string();
    }
    let scope = if groups {
        "groups"
    } else if has("students=") {
        "students"
    } else {
        "everyone"
    };
    format!("{form}&scope={scope}")
}

/// The hidden fields of the review step's confirm form (#120),
/// form-encoded: version, `reviewed` and, to everyone, `reach`.
pub async fn review_fields(b: &Browser<'_>, id: &str) -> String {
    let page = b.get(&format!("/admin/announcements/{id}/review")).await;
    assert_eq!(page.status, 200, "{}", page.body);
    let form = page
        .body
        .split("<form")
        .find(|f| f.contains("/confirm\""))
        .unwrap_or_else(|| panic!("no confirm form: {}", page.body));
    let form = &form[..form.find("</form>").unwrap_or(form.len())];
    form.split("<input type=\"hidden\" ")
        .skip(1)
        .filter_map(|i| {
            let attr = |a: &str| {
                let at = i.find(&format!("{a}=\""))? + a.len() + 2;
                Some(i[at..at + i[at..].find('"')?].to_string())
            };
            Some(format!("{}={}", attr("name")?, attr("value")?))
        })
        .filter(|p| !p.starts_with("_csrf="))
        .collect::<Vec<_>>()
        .join("&")
}

/// Confirms draft `id` through its review step, as Staff do.
pub async fn confirm_reviewed(b: &Browser<'_>, id: &str) -> TestResponse {
    let fields = review_fields(b, id).await;
    b.post(&format!("/admin/announcements/{id}/confirm"), &fields)
        .await
}

/// "ส่งซ้ำ" of `id` seen at `version`, confirmed as Staff do: the ask page
/// first, then `confirm=yes` with the reach it shows to everyone (#120).
pub async fn resend_confirmed(b: &Browser<'_>, id: &str, version: i64) -> TestResponse {
    let path = format!("/admin/announcements/{id}/resend");
    let ask = b.post(&path, &format!("version={version}")).await;
    assert_eq!(ask.status, 200, "{}", ask.body);
    let reach = ask
        .body
        .split("name=\"reach\" value=\"")
        .nth(1)
        .and_then(|r| r.split('"').next())
        .map(|n| format!("&reach={n}"))
        .unwrap_or_default();
    b.post(&path, &format!("version={version}&confirm=yes{reach}"))
        .await
}
