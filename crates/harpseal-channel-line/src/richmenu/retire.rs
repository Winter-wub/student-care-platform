//! Retiring the previous revisions after a re-link (#66): LINE's batch
//! control moves whoever is still on them, and they are deleted only once
//! LINE reports the batch `succeeded` (the rules are in
//! [`crate::richmenu`]).

use harpseal::{Error, Menu, MenuKey, MenuRetire, Result};
use http::Method;
use serde_json::json;
use sha2::{Digest, Sha256};

use super::name::is_salt;
use super::{fail, keeper, rich_menu};
use crate::LineChannel;
use crate::api::{API, answer, parse, path_id};

/// Seconds before the first poll of a batch's progress; each later poll
/// waits twice as long, up to [`PROGRESS_MAX_POLL_SECS`]. LINE allows 100
/// polls an hour per channel, and its own example batch took about 1.5
/// hours, so polling every minute would spend the budget on one batch.
pub const PROGRESS_FIRST_POLL_SECS: u32 = 300;
/// Longest wait between two polls.
pub const PROGRESS_MAX_POLL_SECS: u32 = 3600;
/// Seconds to wait after a 429 on the batch endpoint: LINE allows 3
/// batch requests an hour, one every 20 minutes.
pub const BATCH_RETRY_SECS: u32 = 1200;
/// Prefix of the token of a pending retire.
const TOKEN_PREFIX: &str = "line-retire/2";

/// Seconds before poll number `polls` (0 for the first).
pub fn poll_delay(polls: u32) -> u32 {
    PROGRESS_FIRST_POLL_SECS
        .saturating_mul(1 << polls.min(4))
        .min(PROGRESS_MAX_POLL_SECS)
}

/// A batch still pending: LINE's request id, the rich menus it moves
/// users off, and how many polls it has had.
#[derive(Debug, PartialEq, Eq)]
struct Pending {
    request: String,
    from: Vec<String>,
    polls: u32,
}

impl Pending {
    fn token(&self) -> String {
        format!(
            "{TOKEN_PREFIX} {} {} {}",
            self.request,
            self.from.join(","),
            self.polls
        )
    }
    fn wait(self) -> MenuRetire {
        let retry_after_secs = poll_delay(self.polls);
        MenuRetire::Pending {
            token: self.token(),
            retry_after_secs,
        }
    }
}

/// Whether `m` names a version this Deployment may retire: a revision
/// from 1 with the Deployment's salt. Revision 0 and unsalted names may
/// be shared with another Deployment on the same channel token.
fn salted(m: &Menu) -> bool {
    m.revision() > 0 && m.salt().is_some_and(is_salt)
}

impl LineChannel {
    /// First finishes the batch `pending` (from this or an earlier run of
    /// the Locale): still `ongoing` waits; `succeeded` deletes the rich
    /// menus it moved users off; `failed` or unknown deletes nothing.
    /// Then moves every account still on a rich menu of `previous` (same
    /// Menu key, this generation, salted, exactly the name each revision
    /// had) to `menu`'s with one new batch. Nothing left: `Done`.
    pub(crate) async fn retire(
        &self,
        menu: &Menu,
        previous: &[Menu],
        pending: Option<&str>,
    ) -> Result<MenuRetire> {
        if let Some(p) = pending.and_then(read_token) {
            match self.progress(&p.request).await?.as_deref() {
                Some("ongoing") => {
                    return Ok(Pending {
                        polls: p.polls + 1,
                        ..p
                    }
                    .wait());
                }
                Some("succeeded") => {
                    for id in &p.from {
                        self.delete_or_gone(id).await?;
                    }
                }
                // `failed` (some users not moved: they are still on
                // `from`, and the batch below moves them again) or no
                // longer known: delete nothing.
                _ => {}
            }
        }
        let Some(key) = menu.key().filter(|k| k.as_str() != MenuKey::LOCKED) else {
            return Ok(MenuRetire::Done);
        };
        if !salted(menu) {
            return Ok(MenuRetire::Done);
        }
        let generation = self.cfg.menu_generation();
        let image = &self.cfg.unlocked_menu_image;
        let (name, obj) = rich_menu(Some((key, generation)), menu, image)?;
        let mut names = Vec::new();
        for p in previous
            .iter()
            .filter(|p| p.key() == Some(key) && salted(p))
        {
            let (n, _) = rich_menu(Some((key, generation)), p, image)?;
            if n != name {
                names.push(n);
            }
        }
        if names.is_empty() {
            return Ok(MenuRetire::Done);
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
        Ok(Pending {
            request,
            from,
            polls: 0,
        }
        .wait())
    }

    /// `POST /v2/bot/richmenu/batch`: link `from` -> `to` for every user;
    /// the request id LINE gave it. The resume key is derived from the
    /// operations, so the same batch sent again only redoes the users not
    /// done.
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

/// A token this adapter wrote; anything else is ignored.
fn read_token(token: &str) -> Option<Pending> {
    let mut parts = token.split(' ');
    if parts.next()? != TOKEN_PREFIX {
        return None;
    }
    let request = parts.next()?.to_string();
    let from: Vec<String> = parts.next()?.split(',').map(str::to_string).collect();
    let polls = parts.next()?.parse().ok()?;
    let ok = parts.next().is_none()
        && path_id(&request).is_ok()
        && from.iter().all(|id| path_id(id).is_ok());
    ok.then_some(Pending {
        request,
        from,
        polls,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tokens_round_trip_and_strangers_are_refused() {
        let p = Pending {
            request: "req-1".into(),
            from: vec!["richmenu-a".into(), "richmenu-b".into()],
            polls: 2,
        };
        assert_eq!(read_token(&p.token()), Some(p));
        assert_eq!(read_token("line-resume/2 x y 0"), None);
        assert_eq!(read_token("line-retire/1 req-1 richmenu-a"), None);
        assert_eq!(
            read_token(&format!("{TOKEN_PREFIX} req/1 richmenu-a 0")),
            None
        );
        assert_eq!(
            read_token(&format!("{TOKEN_PREFIX} req-1 richmenu-a")),
            None
        );
    }

    #[test]
    fn polls_back_off_to_an_hour() {
        let d: Vec<u32> = (0..7).map(poll_delay).collect();
        assert_eq!(d, [300, 600, 1200, 2400, 3600, 3600, 3600]);
    }
}
