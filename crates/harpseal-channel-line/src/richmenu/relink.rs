//! The Menu re-link after a label save (#66): bulk link to the new
//! revision, then retire the previous revisions through LINE's batch
//! control, deleting them only once LINE reports the batch succeeded (the
//! rules are in [`crate::richmenu`]).

use harpseal::{ChannelAccountId, Error, Menu, MenuKey, MenuRelinked, MenuRetire, Result};
use http::Method;
use serde_json::json;
use sha2::{Digest, Sha256};

use super::{fail, keeper, rich_menu};
use crate::LineChannel;
use crate::api::{API, answer, parse, path_id};

/// Most user ids in one `POST /v2/bot/richmenu/bulk/link`
/// (<https://developers.line.biz/en/reference/messaging-api/#link-rich-menu-to-users>).
pub const BULK_LINK_MAX: usize = 500;
/// Accounts the Kernel hands one re-link call (5 bulk calls).
pub const RELINK_CHUNK: usize = 2500;
/// Seconds between polls of a batch's progress (LINE allows 100 polls
/// an hour).
pub const PROGRESS_POLL_SECS: u32 = 60;
/// Seconds to wait after a 429 on the batch endpoint: LINE allows 3
/// batch requests an hour, one every 20 minutes.
pub const BATCH_RETRY_SECS: u32 = 1200;
/// Prefix of the token of a pending retire.
const TOKEN_PREFIX: &str = "line-retire/1";

impl LineChannel {
    /// Links every account in `to` to `menu`'s rich menu,
    /// [`BULK_LINK_MAX`] per call. A Menu without a key or entries goes
    /// account by account through `set_menu`.
    pub(crate) async fn relink(&self, to: &[ChannelAccountId], menu: &Menu) -> MenuRelinked {
        let generation = self.cfg.menu_generation();
        let (Some(key), false) = (menu.key(), menu.entries.is_empty()) else {
            for (done, account) in to.iter().enumerate() {
                if let Err(e) = self.rich_menu_set(account, Some(menu)).await {
                    return MenuRelinked::stopped(done, e);
                }
            }
            return MenuRelinked::all(to.len());
        };
        let image = &self.cfg.unlocked_menu_image;
        let named = rich_menu(Some((key, generation)), menu, image);
        let (name, obj) = match named {
            Ok(n) => n,
            Err(e) => return MenuRelinked::stopped(0, e),
        };
        let mut found = match self.rich_menu_id(&name, &obj, image, true).await {
            Ok(f) => f,
            Err(e) => return MenuRelinked::stopped(0, e),
        };
        for (i, group) in to.chunks(BULK_LINK_MAX).enumerate() {
            let done = i * BULK_LINK_MAX;
            let mut linked = self.bulk_link(group, &found.id).await;
            if matches!(&linked, Ok(Err(404))) {
                // Deleted behind our back: find or create it again, once.
                self.menus.forget(&name);
                found = match self.rich_menu_id(&name, &obj, image, false).await {
                    Ok(f) => f,
                    Err(e) => return MenuRelinked::stopped(done, e),
                };
                linked = self.bulk_link(group, &found.id).await;
            }
            if let Err(e) = linked.and_then(|r| {
                r.map_err(|s| Error::Platform(format!("LINE bulk link answered {s}")))
            }) {
                return MenuRelinked::stopped(done, e);
            }
        }
        MenuRelinked::all(to.len())
    }

    /// One bulk link: `Ok(Err(404))` when the rich menu is gone.
    async fn bulk_link(
        &self,
        group: &[ChannelAccountId],
        id: &str,
    ) -> Result<std::result::Result<(), u16>> {
        let users = group
            .iter()
            .map(|a| path_id(&a.external_id).map(str::to_string))
            .collect::<Result<Vec<_>>>()?;
        let body = json!({ "richMenuId": id, "userIds": users });
        match self
            .post_json(&format!("{API}/v2/bot/richmenu/bulk/link"), &body)
            .await?
        {
            Ok(_) => Ok(Ok(())),
            Err(f) if f.status == 404 => Ok(Err(404)),
            Err(f) => Err(fail(f)),
        }
    }

    /// Moves every account still on a rich menu of `previous` (same Menu
    /// key, this generation, exactly the name each revision had) to
    /// `menu`'s, waits for LINE's progress to say `succeeded`, then
    /// deletes them.
    pub(crate) async fn retire(
        &self,
        menu: &Menu,
        previous: &[Menu],
        pending: Option<&str>,
    ) -> Result<MenuRetire> {
        let Some(key) = menu.key().filter(|k| k.as_str() != MenuKey::LOCKED) else {
            return Ok(MenuRetire::Done);
        };
        if let Some((request, from)) = pending.and_then(read_token) {
            match self.progress(&request).await? {
                Some(phase) if phase == "succeeded" => {
                    for id in &from {
                        self.delete_or_gone(id).await?;
                    }
                    return Ok(MenuRetire::Done);
                }
                Some(phase) if phase == "ongoing" => {
                    return Ok(MenuRetire::Pending {
                        token: pending.unwrap_or_default().to_string(),
                        retry_after_secs: PROGRESS_POLL_SECS,
                    });
                }
                // `failed`, or unknown/expired: send the batch again (its
                // resume key only redoes the users not done).
                _ => {}
            }
        }
        let generation = self.cfg.menu_generation();
        let image = &self.cfg.unlocked_menu_image;
        let (name, obj) = rich_menu(Some((key, generation)), menu, image)?;
        let mut names = Vec::new();
        for p in previous.iter().filter(|p| p.key() == Some(key)) {
            let (n, _) = rich_menu(Some((key, generation)), p, image)?;
            if n != name {
                names.push(n);
            }
        }
        let list = self.rich_menu_list().await?;
        let mut from: Vec<String> = list
            .iter()
            .filter(|m| names.contains(&m.name))
            .map(|m| m.id.clone())
            .collect();
        if from.is_empty() {
            return Ok(MenuRetire::Done);
        }
        from.sort();
        let to = match keeper(&list, &name) {
            Some(id) => id,
            None => self.rich_menu_id(&name, &obj, image, false).await?.id,
        };
        let request = self.batch_link(&from, &to).await?;
        Ok(MenuRetire::Pending {
            token: format!("{TOKEN_PREFIX} {request} {}", from.join(",")),
            retry_after_secs: PROGRESS_POLL_SECS,
        })
    }

    /// `POST /v2/bot/richmenu/batch`: link `from` -> `to` for every user;
    /// the request id LINE gave it.
    async fn batch_link(&self, from: &[String], to: &str) -> Result<String> {
        let operations: Vec<_> = from
            .iter()
            .map(|f| json!({ "type": "link", "from": f, "to": to }))
            .collect();
        let mut h = Sha256::new();
        h.update(to.as_bytes());
        for f in from {
            h.update(b",");
            h.update(f.as_bytes());
        }
        let digest: String = h.finalize()[..16]
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect();
        let body = json!({
            "operations": operations,
            "resumeRequestKey": format!("harpseal-{digest}"),
        });
        let bytes = serde_json::to_vec(&body).map_err(|e| Error::Platform(e.to_string()))?;
        let url = format!("{API}/v2/bot/richmenu/batch");
        let resp = self
            .send_raw(Method::POST, &url, Some("application/json"), bytes, true)
            .await?;
        let request = resp
            .headers()
            .get("x-line-request-id")
            .and_then(|v| v.to_str().ok())
            .map(str::to_string);
        answer(resp).map_err(|f| match fail(f) {
            Error::RateLimited { retry_after_secs } => Error::RateLimited {
                retry_after_secs: retry_after_secs.max(BATCH_RETRY_SECS),
            },
            e => e,
        })?;
        request
            .filter(|r| path_id(r).is_ok())
            .ok_or_else(|| Error::Platform("LINE accepted a batch without a request id".into()))
    }

    /// The batch's `phase`, or `None` when LINE no longer knows it (404).
    async fn progress(&self, request: &str) -> Result<Option<String>> {
        let url = format!(
            "{API}/v2/bot/richmenu/progress/batch?requestId={}",
            path_id(request)?
        );
        match self.call(Method::GET, &url, None, Vec::new()).await? {
            Ok(body) => Ok(parse(&body)["phase"].as_str().map(str::to_string)),
            Err(f) if f.status == 404 => Ok(None),
            Err(f) => Err(fail(f)),
        }
    }

    /// `DELETE /v2/bot/richmenu/{id}`; a 404 (already gone) is fine.
    async fn delete_or_gone(&self, id: &str) -> Result<()> {
        let url = format!("{API}/v2/bot/richmenu/{}", path_id(id)?);
        match self.call(Method::DELETE, &url, None, Vec::new()).await? {
            Ok(_) => Ok(()),
            Err(f) if f.status == 404 => Ok(()),
            Err(f) => Err(fail(f)),
        }
    }
}

/// `(request id, rich menu ids)` of a token this adapter wrote.
fn read_token(token: &str) -> Option<(String, Vec<String>)> {
    let mut parts = token.split(' ');
    if parts.next()? != TOKEN_PREFIX {
        return None;
    }
    let request = parts.next()?.to_string();
    let from: Vec<String> = parts.next()?.split(',').map(str::to_string).collect();
    let ok = parts.next().is_none()
        && path_id(&request).is_ok()
        && from.iter().all(|id| path_id(id).is_ok());
    ok.then_some((request, from))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tokens_round_trip_and_strangers_are_refused() {
        let t = format!("{TOKEN_PREFIX} req-1 richmenu-a,richmenu-b");
        assert_eq!(
            read_token(&t),
            Some((
                "req-1".into(),
                vec!["richmenu-a".into(), "richmenu-b".into()]
            ))
        );
        assert_eq!(read_token("line-resume/2 x y"), None);
        assert_eq!(
            read_token(&format!("{TOKEN_PREFIX} req/1 richmenu-a")),
            None
        );
        assert_eq!(read_token(&format!("{TOKEN_PREFIX} req-1")), None);
    }
}
