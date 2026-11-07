//! The Menu re-link after a label save (#66): every account to the new
//! revision with `POST /v2/bot/richmenu/bulk/link` (the rules are in
//! [`crate::richmenu`]; the retire is in [`super::retire`]).

use harpseal::{ChannelAccountId, Error, Menu, MenuRelinked, Result};
use serde_json::json;

use super::{fail, rich_menu};
use crate::LineChannel;
use crate::api::{API, Failure};

/// Most user ids in one `POST /v2/bot/richmenu/bulk/link`
/// (<https://developers.line.biz/en/reference/messaging-api/#link-rich-menu-to-users>).
pub const BULK_LINK_MAX: usize = 500;
/// Accounts the Kernel hands one re-link call: 5 bulk calls, about 8
/// subrequests with the lookup, well under any Workers limit.
pub const RELINK_CHUNK: usize = 2500;
/// Most per-user links one re-link call makes when a bulk call is refused
/// for a reason that names no user: the call then stops early (no error)
/// and the Kernel continues after them in its next Job run, so a run stays
/// under the 50 subrequests of a Workers free-plan invocation.
pub const ONE_BY_ONE_MAX: usize = 40;

/// What one bulk call said.
enum Bulk {
    Linked,
    /// The rich menu is gone (404).
    Gone,
    /// LINE refused the user id at this position (400 `userIds[n]`).
    BadUser(usize),
    /// Another 400, not about one user id.
    Refused,
}

/// A LINE user id: `U` and 32 lowercase hex digits. Anything else would
/// make LINE refuse the whole bulk call (400), so it is skipped first.
pub(crate) fn is_user_id(id: &str) -> bool {
    id.len() == 33
        && id.starts_with('U')
        && id[1..]
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

impl LineChannel {
    /// Links every account in `to` to `menu`'s rich menu,
    /// [`BULK_LINK_MAX`] per call. An id that is no LINE user id, or that
    /// LINE refuses, is skipped ([`Error::Invalid`]); a 400 that names no
    /// user goes account by account. A Menu without a key or entries goes
    /// account by account through `set_menu`.
    pub(crate) async fn relink(&self, to: &[ChannelAccountId], menu: &Menu) -> MenuRelinked {
        let mut got = MenuRelinked::all(to.len());
        let mut valid = Vec::new();
        for (i, a) in to.iter().enumerate() {
            if is_user_id(&a.external_id) {
                valid.push(i);
            } else {
                got.skip(i, not_a_user(&a.external_id));
            }
        }
        let generation = self.cfg.menu_generation();
        let (Some(key), false) = (menu.key(), menu.entries.is_empty()) else {
            for i in valid {
                match self.rich_menu_set(&to[i], Some(menu)).await {
                    Ok(()) => {}
                    Err(e) if matches!(e, Error::Unreachable) => got.skip(i, e),
                    Err(e) => return stop(got, i, e),
                }
            }
            return got;
        };
        let image = &self.cfg.unlocked_menu_image;
        let (name, obj) = match rich_menu(Some((key, generation)), menu, image) {
            Ok(n) => n,
            Err(e) => return MenuRelinked::stopped(0, e),
        };
        let mut found = match self.rich_menu_id(&name, &obj, image, true).await {
            Ok(f) => f,
            Err(e) => return MenuRelinked::stopped(0, e),
        };
        let mut recreated = false;
        let mut budget = ONE_BY_ONE_MAX;
        for group in valid.chunks(BULK_LINK_MAX) {
            let first = group[0];
            let mut group = group.to_vec();
            while !group.is_empty() {
                let users: Vec<&str> = group.iter().map(|i| to[*i].external_id.as_str()).collect();
                match self.bulk_link(&users, &found.id).await {
                    Ok(Bulk::Linked) => break,
                    Ok(Bulk::Gone) if !recreated => {
                        // Deleted behind our back: find or create it again,
                        // once per call.
                        recreated = true;
                        self.menus.forget(&name);
                        found = match self.rich_menu_id(&name, &obj, image, false).await {
                            Ok(f) => f,
                            Err(e) => return stop(got, first, e),
                        };
                    }
                    Ok(Bulk::BadUser(n)) if n < group.len() => {
                        let i = group.remove(n);
                        got.skip(i, not_a_user(&to[i].external_id));
                    }
                    Ok(Bulk::Refused) => {
                        let take = group.len().min(budget);
                        let some = &group[..take];
                        if let Err(e) = self.one_by_one(to, some, &found.id, &mut got).await {
                            return stop(got, first, e);
                        }
                        budget -= take;
                        if take < group.len() {
                            // Out of budget: the Kernel goes on from here.
                            return early(got, group[take]);
                        }
                        break;
                    }
                    Ok(_) => {
                        let e = Error::Platform("LINE bulk link refused the rich menu".into());
                        return stop(got, first, e);
                    }
                    Err(e) => return stop(got, first, e),
                }
            }
        }
        got
    }

    /// One bulk link of `users` to rich menu `id`.
    async fn bulk_link(&self, users: &[&str], id: &str) -> Result<Bulk> {
        let body = json!({ "richMenuId": id, "userIds": users });
        match self
            .post_json(&format!("{API}/v2/bot/richmenu/bulk/link"), &body)
            .await?
        {
            Ok(_) => Ok(Bulk::Linked),
            Err(f) if f.status == 404 => Ok(Bulk::Gone),
            Err(f) if f.status == 400 => Ok(bad_user(&f).map_or(Bulk::Refused, Bulk::BadUser)),
            Err(f) => Err(fail(f)),
        }
    }

    /// `group` (indices in `to`) linked one by one. A 400 about the user
    /// id (LINE: "The value for the 'userId' parameter is invalid",
    /// <https://developers.line.biz/en/reference/messaging-api/#link-rich-menu-to-user-error-response>)
    /// skips that account; any other 400 is about the rich menu (no
    /// image, a bad id) and stops, like anything else.
    async fn one_by_one(
        &self,
        to: &[ChannelAccountId],
        group: &[usize],
        id: &str,
        got: &mut MenuRelinked,
    ) -> Result<()> {
        for i in group {
            match self.link(&to[*i].external_id, id).await? {
                Ok(()) => {}
                Err(f) if f.status == 400 && about_user(&f) => {
                    got.skip(*i, Error::Invalid(f.message));
                }
                Err(f) if f.status == 400 => {
                    return Err(Error::Platform(format!(
                        "LINE refused rich menu {id}: {}",
                        f.message
                    )));
                }
                Err(f) => return Err(fail(f)),
            }
        }
        Ok(())
    }
}

/// Whether a 400 on a per-user link is about the user id, not the menu.
pub(super) fn about_user(f: &Failure) -> bool {
    f.message.contains("'userId'")
}

/// The position in `userIds` a 400 names (`'userIds[3]'`), if any.
fn bad_user(f: &Failure) -> Option<usize> {
    let rest = f.message.split("userIds[").nth(1)?;
    rest.split(']').next()?.parse().ok()
}

fn not_a_user(id: &str) -> Error {
    Error::Invalid(format!("not a LINE user id: {id:?}"))
}

/// `got` done up to account `at` (not included), without an error: the
/// Kernel continues at `to[at..]`.
fn early(mut got: MenuRelinked, at: usize) -> MenuRelinked {
    got.done = at;
    got.skipped.retain(|(i, _)| *i < at);
    got
}

/// `got` stopped at account `at` by `e`; skips past `at` are dropped
/// (they are done again on the retry).
fn stop(mut got: MenuRelinked, at: usize, e: Error) -> MenuRelinked {
    got.done = at;
    got.error = Some(e);
    got.skipped.retain(|(i, _)| *i < at);
    got
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn user_ids_and_refused_positions() {
        assert!(is_user_id("U5ea1ba7000000000000000000000c0de"));
        assert!(!is_user_id("U5EA1BA7000000000000000000000C0DE"));
        assert!(!is_user_id("C5ea1ba7000000000000000000000c0de"));
        assert!(!is_user_id("U5ea1ba7"));
        let f = |m: &str| Failure::for_test(400, m);
        let named =
            "The property, 'userIds[3]', in the request body is invalid (line: -, column: -)";
        assert_eq!(bad_user(&f(named)), Some(3));
        assert_eq!(bad_user(&f("invalid richMenuId")), None);
        assert!(about_user(&f(
            "The value for the 'userId' parameter is invalid"
        )));
        assert!(!about_user(&f(
            "The value for the 'richMenuId' parameter is invalid"
        )));
        assert!(!about_user(&f("richmenu has no image")));
    }
}
