//! After a link: delete duplicate and superseded rich menus (the rules are
//! in [`crate::richmenu`]).

use super::{Found, Listed, MAX_DELETES, keeper, name_key};
use crate::LineChannel;

impl LineChannel {
    /// Cleans up after `user` was linked to `found` (named `name`, Menu key
    /// and generation `tag`). Nothing on a cache hit; never fails.
    pub(super) async fn tidy(
        &self,
        user: &str,
        tag: Option<(&str, u32)>,
        name: &str,
        found: Found,
    ) {
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
            // account onto it, then ours is the duplicate. If that fails,
            // delete nothing: our account stays on ours.
            if !matches!(self.link(user, &keep).await, Ok(Ok(()))) {
                return;
            }
            self.menus.put(name, &keep);
        }
        for id in doomed(&list, tag, name, &keep) {
            // A failure (already deleted, rate limited) is left for a later
            // cold call.
            self.delete_rich_menu(&id).await;
        }
    }
}

/// The ids to delete: twins of `name` other than `keep`, and versions with
/// the same Menu key and a lower generation than `tag`'s; at most
/// [`MAX_DELETES`], the first in list order. LINE does not specify that
/// order, so which ones go first is arbitrary; the rest go on later calls.
fn doomed(list: &[Listed], tag: Option<(&str, u32)>, name: &str, keep: &str) -> Vec<String> {
    let superseded = |n: &str| match (tag, name_key(n).flatten()) {
        (Some((key, generation)), Some((k, g))) => k == key && g < generation,
        _ => false,
    };
    list.iter()
        .filter(|m| m.id != keep)
        .filter(|m| m.name == name || superseded(&m.name))
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

    fn h(c: char) -> String {
        c.to_string().repeat(32)
    }

    #[test]
    fn only_twins_and_lower_generations_of_the_same_key_go() {
        let me = format!("harpseal-th-2-{}", h('a'));
        let list = [
            l("richmenu-1", &me),
            l("richmenu-2", &me),
            l("richmenu-3", &format!("harpseal-th-1-{}", h('b'))),
            l("richmenu-4", &format!("harpseal-th-2-{}", h('c'))),
            l("richmenu-5", &format!("harpseal-th-3-{}", h('d'))),
            l("richmenu-6", &format!("harpseal-en-1-{}", h('e'))),
            l("richmenu-7", &format!("harpseal-locked-1-{}", h('f'))),
            l("richmenu-8", &format!("harpseal-{}", h('a'))),
            l("richmenu-9", "harpseal-th-backup"),
            l("richmenu-10", "made-by-hand"),
        ];
        let got = doomed(&list, Some(("th", 2)), &me, "richmenu-1");
        assert_eq!(got, ["richmenu-2", "richmenu-3"]);
        let unkeyed = format!("harpseal-{}", h('a'));
        let got = doomed(&list, None, &unkeyed, "richmenu-8");
        assert!(got.is_empty(), "{got:?}");
    }

    #[test]
    fn deletes_are_bounded() {
        let list: Vec<Listed> = (0..50)
            .map(|i| {
                l(
                    &format!("richmenu-{i:02}"),
                    &format!("harpseal-th-{i}-{}", h('a')),
                )
            })
            .collect();
        let me = format!("harpseal-th-99-{}", h('b'));
        let got = doomed(&list, Some(("th", 99)), &me, "richmenu-new");
        assert_eq!(got.len(), MAX_DELETES);
    }
}
