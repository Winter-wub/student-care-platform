//! After a link: delete duplicate and superseded rich menus (the rules are
//! in [`crate::richmenu`]).

use super::{Found, Listed, MAX_DELETES, keeper, name_key};
use crate::LineChannel;

impl LineChannel {
    /// Cleans up after `user` was linked to `found` (named `name`, Menu key
    /// `key`). Nothing on a cache hit; never fails.
    pub(super) async fn tidy(&self, user: &str, key: Option<&str>, name: &str, found: Found) {
        let list = match (found.list, found.created) {
            (None, _) => return,
            // Created now: list again, to see a twin created meanwhile.
            (Some(_), true) => match self.rich_menu_list().await {
                Ok(l) => l,
                Err(_) => return,
            },
            (Some(l), false) => l,
        };
        let keep = keeper(&list, name).unwrap_or_else(|| found.id.clone());
        if keep != found.id {
            // Another isolate created a twin with a smaller id: move this
            // account onto it, then ours is the duplicate.
            if !matches!(self.link(user, &keep).await, Ok(Ok(()))) {
                return;
            }
            self.menus.put(name, &keep);
        }
        for id in doomed(&list, key, name, &keep) {
            // A failure (already deleted, say) is left for the next cold call.
            self.delete_rich_menu(&id).await;
        }
    }
}

/// The ids to delete: twins of `name` other than `keep`, and other versions
/// under Menu key `key`; at most [`MAX_DELETES`].
fn doomed(list: &[Listed], key: Option<&str>, name: &str, keep: &str) -> Vec<String> {
    list.iter()
        .filter(|m| m.id != keep)
        .filter(|m| m.name == name || (key.is_some() && name_key(&m.name).flatten() == key))
        .take(MAX_DELETES)
        .map(|m| m.id.clone())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn l(id: &str, name: &str) -> Listed {
        Listed {
            id: id.into(),
            name: name.into(),
        }
    }

    #[test]
    fn only_twins_and_versions_of_the_same_key_go() {
        let list = [
            l("richmenu-1", "harpseal-th-aaaa"),
            l("richmenu-2", "harpseal-th-aaaa"),
            l("richmenu-3", "harpseal-th-bbbb"),
            l("richmenu-4", "harpseal-en-cccc"),
            l("richmenu-5", "harpseal-locked-dddd"),
            l("richmenu-6", "harpseal-eeee"),
            l("richmenu-7", "made-by-hand"),
            l("richmenu-8", "harpseal-Bad_Key-ffff"),
        ];
        let got = doomed(&list, Some("th"), "harpseal-th-aaaa", "richmenu-1");
        assert_eq!(got, ["richmenu-2", "richmenu-3"]);
        let got = doomed(&list, None, "harpseal-eeee", "richmenu-6");
        assert!(got.is_empty(), "{got:?}");
    }

    #[test]
    fn deletes_are_bounded() {
        let list: Vec<Listed> = (0..50)
            .map(|i| l(&format!("richmenu-{i:02}"), &format!("harpseal-th-{i:02}")))
            .collect();
        let got = doomed(&list, Some("th"), "harpseal-th-00", "richmenu-00");
        assert_eq!(got.len(), MAX_DELETES);
    }
}
