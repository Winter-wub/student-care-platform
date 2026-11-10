//! What a bulk action on the Students list acts on (#145): the rows
//! ticked, every row of the page, or every Student matching the list's
//! filter. The filter travels as the search itself (`q`, `missing`), never
//! as a list of ids, and is read again on the server at each step.

use maud::{Markup, html};

use super::read;
use crate::config::RosterConfig;
use crate::port_types::{Statement, Value};
use crate::{Result, Services};

/// Most Students one bulk action names: a hand-picked Audience's limit
/// ([`crate::audience::MAX_STUDENTS`]), for Tags and announcements alike.
pub(crate) const MAX_BULK: usize = crate::audience::MAX_STUDENTS;

/// Most Students one bulk unlink names: each revoke tells the Student in
/// the chat, inside the request.
pub(crate) const MAX_UNLINK: usize = 100;

/// Which rows a bulk action takes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Which {
    /// The rows ticked (`student=<id>`, repeated).
    Picked,
    /// Every row of list page `page`.
    Page,
    /// Every Student matching `q` and `missing`.
    Filter,
}

impl Which {
    pub(super) fn as_str(self) -> &'static str {
        match self {
            Which::Picked => "picked",
            Which::Page => "page",
            Which::Filter => "filter",
        }
    }
}

/// A selection as the list's bulk form posts it.
#[derive(Debug, Clone)]
pub(super) struct Selection {
    pub(super) q: String,
    pub(super) missing: bool,
    pub(super) page: i64,
    pub(super) which: Which,
    /// Student ids ticked, repeats dropped, in order.
    pub(super) picked: Vec<String>,
}

impl Selection {
    /// From the posted pairs: `q`, `missing`, `page`, `which` and every
    /// `student`. An unknown `which` is [`Which::Picked`].
    pub(super) fn from_pairs(pairs: &[(String, String)]) -> Self {
        let get = |k: &str| {
            pairs
                .iter()
                .find(|(n, _)| n == k)
                .map_or("", |(_, v)| v.as_str())
        };
        let which = match get("which") {
            "page" => Which::Page,
            "filter" => Which::Filter,
            _ => Which::Picked,
        };
        let mut picked: Vec<String> = Vec::new();
        for (_, id) in pairs.iter().filter(|(n, _)| n == "student") {
            let id = id.trim();
            if !id.is_empty() && !picked.iter().any(|p| p == id) {
                picked.push(id.to_string());
            }
        }
        Self {
            q: get("q").trim().chars().take(100).collect(),
            missing: matches!(get("missing"), "1" | "on" | "true"),
            page: get("page").parse().unwrap_or(1),
            which,
            picked,
        }
    }

    /// The hidden fields that post this selection again (a review step).
    /// A page posts the rows it showed (`ids`) as ticked rows, so a page
    /// that shifts meanwhile (a roster import) cannot swap them.
    pub(super) fn hidden(&self, ids: &[String]) -> Markup {
        let (which, rows) = match self.which {
            Which::Page => (Which::Picked, ids),
            w => (w, self.picked.as_slice()),
        };
        html! {
            input type="hidden" name="q" value=(self.q);
            @if self.missing { input type="hidden" name="missing" value="1"; }
            input type="hidden" name="page" value=(self.page);
            input type="hidden" name="which" value=(which.as_str());
            @if which == Which::Picked {
                @for id in rows { input type="hidden" name="student" value=(id); }
            }
        }
    }

    /// The list page this selection came from.
    pub(super) fn list_href(&self) -> String {
        let q = super::form::urlencode(&self.q);
        let m = if self.missing { "&missing=1" } else { "" };
        format!("/admin/students?q={q}{m}&page={}", self.page.max(1))
    }
}

/// The Students a selection names.
#[derive(Debug, Default)]
pub(super) struct Chosen {
    /// Student ids, by student code; empty when `total` is over the cap.
    pub(super) ids: Vec<String>,
    /// Their student codes, in the same order.
    pub(super) codes: Vec<String>,
    /// How many Students the selection names in all.
    pub(super) total: usize,
}

impl Chosen {
    /// Whether the selection names more than `cap` Students.
    pub(super) fn over(&self, cap: usize) -> bool {
        self.total > cap
    }
}

/// Only active Students (an announcement), or everyone selected.
const ACTIVE: &str = "status = 'active'";

/// The Students `sel` names now, read on the server: ids and codes when
/// there are at most `cap` of them, only the total otherwise.
/// `active_only` leaves out Students who left (an announcement).
pub(super) async fn resolve(
    s: &Services,
    roster: &RosterConfig,
    sel: &Selection,
    cap: usize,
    active_only: bool,
) -> Result<Chosen> {
    match sel.which {
        Which::Page => {
            let found = read::search_with(s, &sel.q, sel.page, Some(roster), sel.missing).await?;
            let (ids, codes) = found
                .rows
                .into_iter()
                .filter(|r| !active_only || r.status == "active")
                .map(|r| (r.id, r.code))
                .unzip();
            Ok(capped(ids, codes, cap))
        }
        Which::Filter => {
            let (filter, binds) = read::list_filter(s, &sel.q, Some(roster), sel.missing).await?;
            let filter = if active_only {
                format!("{filter} AND {ACTIVE}")
            } else {
                filter
            };
            let with = |sql: String| {
                let mut st = Statement::new(sql);
                st.params.extend(binds.iter().cloned());
                st
            };
            let n = with(format!("SELECT count(*) AS n FROM student {filter}"));
            let total = read::int(&s.db.query(n).await?, 0, "n").unwrap_or(0).max(0) as usize;
            if total > cap || total == 0 {
                return Ok(Chosen {
                    total,
                    ..Chosen::default()
                });
            }
            let pick = with(format!(
                "SELECT id, student_code FROM student {filter} ORDER BY student_code, id LIMIT ?"
            ))
            .bind(cap as i64);
            let rows = s.db.query(pick).await?;
            let (ids, codes) = id_codes(&rows);
            Ok(capped(ids, codes, cap))
        }
        Which::Picked => {
            if sel.picked.len() > cap {
                return Ok(Chosen {
                    total: sel.picked.len(),
                    ..Chosen::default()
                });
            }
            let list = serde_json::to_string(&sel.picked).unwrap_or_else(|_| "[]".into());
            let active = if active_only {
                format!(" AND {ACTIVE}")
            } else {
                String::new()
            };
            let pick = Statement::new(format!(
                "SELECT id, student_code FROM student \
                 WHERE id IN (SELECT value FROM json_each(?)){active} ORDER BY student_code, id"
            ))
            .bind(list);
            let rows = s.db.query(pick).await?;
            let (ids, codes) = id_codes(&rows);
            Ok(capped(ids, codes, cap))
        }
    }
}

/// How many Students `sel` names, counted only: the HTMX count.
pub(super) async fn count(s: &Services, roster: &RosterConfig, sel: &Selection) -> Result<usize> {
    if sel.which == Which::Picked {
        return Ok(sel.picked.len());
    }
    let (filter, binds) = read::list_filter(s, &sel.q, Some(roster), sel.missing).await?;
    let mut n = Statement::new(format!("SELECT count(*) AS n FROM student {filter}"));
    n.params.extend(binds);
    let total = read::int(&s.db.query(n).await?, 0, "n").unwrap_or(0).max(0);
    if sel.which == Which::Filter {
        return Ok(total as usize);
    }
    // The rows of page `page` (clamped as the list clamps it).
    let size = read::PAGE_SIZE;
    let pages = ((total + size - 1) / size).max(1);
    let page = sel.page.clamp(1, pages);
    Ok((total - (page - 1) * size).clamp(0, size) as usize)
}

fn id_codes(rows: &crate::port_types::Rows) -> (Vec<String>, Vec<String>) {
    (0..rows.len())
        .map(|i| {
            let t = |c: &str| read::text(rows, i, c).unwrap_or_default();
            (t("id"), t("student_code"))
        })
        .unzip()
}

fn capped(ids: Vec<String>, codes: Vec<String>, cap: usize) -> Chosen {
    let total = ids.len();
    if total > cap {
        return Chosen {
            total,
            ..Chosen::default()
        };
    }
    Chosen { ids, codes, total }
}

/// The student codes of every Student matching the list's filter `q` and
/// `missing` who are active (#145, the "ประกาศถึงที่เลือก" of a whole
/// filter): `None` when none match or more than [`MAX_BULK`] do. The announcement editor
/// reads them on the server, so the link carries the filter, not the
/// list.
pub(crate) async fn codes_matching(
    s: &Services,
    roster: &RosterConfig,
    q: &str,
    missing: bool,
) -> Result<Option<Vec<String>>> {
    let sel = Selection {
        q: q.trim().chars().take(100).collect(),
        missing,
        page: 1,
        which: Which::Filter,
        picked: Vec::new(),
    };
    let chosen = resolve(s, roster, &sel, MAX_BULK, true).await?;
    Ok((!chosen.codes.is_empty()).then_some(chosen.codes))
}

/// A token of the set `ids` (in any order): the review posts it, and a
/// confirm whose set differs is reviewed again (#145).
pub(super) fn set_token(ids: &[String]) -> String {
    let mut sorted: Vec<&str> = ids.iter().map(String::as_str).collect();
    sorted.sort_unstable();
    crate::auth::sha256_hex(sorted.join("\n").as_bytes())
}

/// JSON array of `ids`, for `json_each(?)`.
pub(super) fn json_ids(ids: &[String]) -> Value {
    Value::from(serde_json::to_string(ids).unwrap_or_else(|_| "[]".into()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pairs(list: &[(&str, &str)]) -> Vec<(String, String)> {
        list.iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    #[test]
    fn a_selection_reads_its_fields_and_drops_repeats() {
        let sel = Selection::from_pairs(&pairs(&[
            ("q", "  นก "),
            ("missing", "1"),
            ("page", "3"),
            ("student", "s1"),
            ("student", "s2"),
            ("student", "s1"),
            ("student", " "),
        ]));
        assert_eq!(sel.q, "นก");
        assert!(sel.missing);
        assert_eq!(sel.page, 3);
        assert_eq!(sel.which, Which::Picked);
        assert_eq!(sel.picked, vec!["s1".to_string(), "s2".into()]);
        assert_eq!(
            sel.list_href(),
            "/admin/students?q=%E0%B8%99%E0%B8%81&missing=1&page=3"
        );
    }

    #[test]
    fn which_defaults_to_the_ticked_rows() {
        let page = Selection::from_pairs(&pairs(&[("which", "page")]));
        assert_eq!(page.which, Which::Page);
        let filter = Selection::from_pairs(&pairs(&[("which", "filter")]));
        assert_eq!(filter.which, Which::Filter);
        let odd = Selection::from_pairs(&pairs(&[("which", "everyone")]));
        assert_eq!(odd.which, Which::Picked);
        assert_eq!(odd.page, 1);
    }

    #[test]
    fn the_set_token_ignores_order_but_not_members() {
        let a = set_token(&["s2".into(), "s1".into()]);
        assert_eq!(a, set_token(&["s1".into(), "s2".into()]));
        assert_ne!(a, set_token(&["s1".into(), "s3".into()]));
    }

    #[test]
    fn a_selection_over_the_cap_names_nobody() {
        let c = capped(
            vec!["a".into(), "b".into()],
            vec!["1".into(), "2".into()],
            1,
        );
        assert!(c.over(1) && c.ids.is_empty() && c.total == 2);
        let c = capped(vec!["a".into()], vec!["1".into()], 1);
        assert!(!c.over(1) && c.ids == ["a"]);
    }
}
