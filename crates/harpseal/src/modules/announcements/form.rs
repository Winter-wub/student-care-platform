//! The create / edit form as posted ([`Fields`]), its checks, and the
//! [`Draft`] it becomes.

use std::collections::BTreeMap;

use super::service::Draft;
use super::store::{Row, Texts};
use super::view::{end_of_day, input_date, input_datetime, local_datetime};
use crate::audience::{Audience, MAX_STUDENTS, Range, RangeError, parse_student_ids};
use crate::config::{AttributeType, RosterConfig};

/// Longest title and body, in characters.
pub(crate) const MAX_TITLE: usize = 100;
pub(crate) const MAX_BODY: usize = 2000;

/// The form exactly as typed, so a refused post re-renders unchanged.
#[derive(Debug, Clone, Default)]
pub(crate) struct Fields {
    pub(crate) title_th: String,
    pub(crate) body_th: String,
    pub(crate) title_en: String,
    pub(crate) body_en: String,
    pub(crate) require_ack: bool,
    pub(crate) expires: String,
    /// `now` or `later`.
    pub(crate) when: String,
    pub(crate) send_at: String,
    /// `attr.<key>` values (checkboxes, or one comma-separated text).
    pub(crate) attrs: BTreeMap<String, Vec<String>>,
    /// `attr_min.<key>` and `attr_max.<key>` as typed (#156): a numeric
    /// attribute's bounds.
    pub(crate) ranges: BTreeMap<String, (String, String)>,
    pub(crate) tags: String,
    /// Hand-picked student ids as typed (newline, comma or space separated).
    pub(crate) students: String,
    pub(crate) unreachable: bool,
    /// Who it goes to (#120): `everyone`, `groups` (the attribute and Tag
    /// filters) or `students` (the hand-picked list). Empty filters never
    /// mean everyone silently: see [`Fields::scope_problem`].
    pub(crate) scope: String,
    /// "Notify recipients of the edit" (a sent announcement only; #77).
    pub(crate) notify: bool,
    pub(crate) version: Option<i64>,
    /// The submit button pressed: `draft` or `confirm`.
    pub(crate) submit: String,
}

/// `errors` with each (field, key) once, in first-seen order.
pub(crate) fn unique(
    errors: Vec<(&'static str, &'static str)>,
) -> Vec<(&'static str, &'static str)> {
    let mut out: Vec<(&'static str, &'static str)> = Vec::with_capacity(errors.len());
    for e in errors {
        if !out.contains(&e) {
            out.push(e);
        }
    }
    out
}

fn split(s: &str) -> impl Iterator<Item = String> + '_ {
    s.split(',')
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
}

impl Fields {
    /// From the posted pairs.
    pub(crate) fn from_pairs(pairs: &[(String, String)]) -> Self {
        let mut f = Fields::default();
        for (k, v) in pairs {
            match k.as_str() {
                "title_th" => f.title_th.clone_from(v),
                "body_th" => f.body_th.clone_from(v),
                "title_en" => f.title_en.clone_from(v),
                "body_en" => f.body_en.clone_from(v),
                "require_ack" => f.require_ack = true,
                "expires" => f.expires.clone_from(v),
                "when" => f.when.clone_from(v),
                "send_at" => f.send_at.clone_from(v),
                "tags" => f.tags.clone_from(v),
                "students" => f.students.clone_from(v),
                "unreachable" => f.unreachable = true,
                "scope" => f.scope.clone_from(v),
                "notify" => f.notify = true,
                "version" => f.version = v.parse().ok(),
                "do" => f.submit.clone_from(v),
                other => {
                    if let Some(key) = other.strip_prefix("attr.") {
                        f.attrs.entry(key.to_string()).or_default().extend(split(v));
                    } else if let Some(key) = other.strip_prefix("attr_min.") {
                        f.ranges.entry(key.to_string()).or_default().0 = v.trim().to_string();
                    } else if let Some(key) = other.strip_prefix("attr_max.") {
                        f.ranges.entry(key.to_string()).or_default().1 = v.trim().to_string();
                    }
                }
            }
        }
        if f.when.is_empty() {
            f.when = "now".into();
        }
        f
    }

    /// From a stored announcement, for its edit form.
    pub(crate) fn of(row: &Row, texts: &Texts, offset: i32) -> Self {
        let get = |l: &str| texts.0.get(l).cloned().unwrap_or_default();
        let (title_th, body_th) = get("th");
        let (title_en, body_en) = get("en");
        let mut f = Fields {
            title_th,
            body_th,
            title_en,
            body_en,
            require_ack: row.require_ack,
            expires: row
                .expires_at
                .map(|e| input_date(e, offset))
                .unwrap_or_default(),
            when: if row.send_at.is_some() {
                "later"
            } else {
                "now"
            }
            .into(),
            send_at: row
                .send_at
                .map(|t| input_datetime(t, offset))
                .unwrap_or_default(),
            notify: false,
            version: Some(row.version),
            submit: String::new(),
            ..Fields::default()
        };
        f.set_audience(&row.audience);
        f
    }

    /// The Audience fields set to `a`'s rules, and the choice it reads as
    /// ([`scope_of`]).
    pub(crate) fn set_audience(&mut self, a: &Audience) {
        self.attrs = a.attributes.clone();
        self.ranges = a
            .ranges
            .iter()
            .map(|(k, r)| {
                let b = |x: &Option<String>| x.clone().unwrap_or_default();
                (k.clone(), (b(&r.min), b(&r.max)))
            })
            .collect();
        self.tags = a.tags.join(", ");
        self.students = a.students.join("\n");
        self.unreachable = a.include_unreachable;
        self.scope = scope_of(a).into();
    }

    /// The Audience the form describes (unchecked). The choice keeps only
    /// its own filters (#120): `everyone` none, `groups` the attributes and
    /// Tags, `students` the list, so filters left filled in under another
    /// choice are dropped, never sent to. A post without a choice (an
    /// older page) keeps them all.
    pub(crate) fn audience(&self) -> Audience {
        let mut a = Audience::everyone();
        a.include_unreachable = self.unreachable;
        let (groups, list) = match self.scope.as_str() {
            "everyone" => (false, false),
            "groups" => (true, false),
            "students" => (false, true),
            _ => (true, true),
        };
        if groups {
            a.attributes = self
                .attrs
                .iter()
                .filter(|(_, v)| !v.is_empty())
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect();
            a.ranges = self
                .ranges
                .iter()
                .map(|(k, (lo, hi))| (k.clone(), Range::new(lo, hi)))
                .filter(|(_, r)| !r.is_empty())
                .collect();
            a.tags = split(&self.tags).collect();
        }
        if list {
            a.students = parse_student_ids(&self.students);
        }
        a
    }

    /// Groups and a list together, with no choice made: an Audience saved
    /// before #120 (see [`scope_of`]).
    pub(crate) fn combined(&self) -> bool {
        let ranged = self
            .ranges
            .values()
            .any(|(lo, hi)| !lo.is_empty() || !hi.is_empty());
        let groups =
            self.attrs.values().any(|v| !v.is_empty()) || ranged || !self.tags.trim().is_empty();
        self.scope.is_empty() && groups && !self.students.trim().is_empty()
    }

    /// Why the Audience choice cannot be used (#120), as a catalog key:
    /// no choice made while every filter is empty, `groups` without a
    /// group, `students` without an id. A post without `scope` but with
    /// filters (an older page) keeps its filters.
    pub(crate) fn scope_problem(&self) -> Option<&'static str> {
        let a = self.audience();
        let groups = a.has_groups();
        match self.scope.as_str() {
            "everyone" => None,
            "groups" if !groups => Some("announcements.form.groups_empty"),
            "students" if a.students.is_empty() => Some("announcements.form.students_empty"),
            "groups" | "students" => None,
            _ if is_everyone(&a) => Some("announcements.form.scope_required"),
            _ => None,
        }
    }

    /// The Draft, or every problem as (field, catalog key).
    pub(crate) fn draft(
        &self,
        roster: &RosterConfig,
        offset: i32,
        now: i64,
    ) -> Result<Draft, Vec<(&'static str, &'static str)>> {
        let mut errors = Vec::new();
        let long = |s: &str, n: usize| s.chars().count() > n;
        if self.title_th.trim().is_empty() || self.body_th.trim().is_empty() {
            errors.push(("th", "announcements.form.th_required"));
        }
        let en_title = self.title_en.trim();
        let en_body = self.body_en.trim();
        if en_title.is_empty() != en_body.is_empty() {
            errors.push(("en", "announcements.form.en_both"));
        }
        if [&self.title_th, &self.title_en]
            .iter()
            .any(|t| long(t, MAX_TITLE))
            || [&self.body_th, &self.body_en]
                .iter()
                .any(|b| long(b, MAX_BODY))
        {
            errors.push(("th", "announcements.form.too_long"));
        }
        let expires_at = match self.expires.trim() {
            "" => None,
            e => match end_of_day(e, offset) {
                Some(t) if t > now => Some(t),
                _ => {
                    errors.push(("expires", "announcements.form.bad_expires"));
                    None
                }
            },
        };
        let send_at = if self.when == "later" {
            match local_datetime(&self.send_at, offset) {
                Some(t) if t > now => Some(t),
                _ => {
                    errors.push(("send_at", "announcements.form.bad_send_at"));
                    None
                }
            }
        } else {
            None
        };
        errors.extend(self.audience_errors(roster));
        let audience = self.audience();
        if !errors.is_empty() {
            return Err(unique(errors));
        }
        let mut texts = vec![(
            "th".to_string(),
            self.title_th.trim().to_string(),
            self.body_th.trim().to_string(),
        )];
        if !en_title.is_empty() {
            texts.push(("en".into(), en_title.to_string(), en_body.to_string()));
        }
        Ok(Draft {
            texts,
            audience,
            require_ack: self.require_ack,
            expires_at,
            send_at,
            notify: self.notify,
            files: Default::default(),
        })
    }

    /// The Audience's problems as (`audience`, catalog key): the choice
    /// ([`Fields::scope_problem`]), an attribute or value `[roster]` does
    /// not have, too many hand-picked ids. A saved Audience (#144) is
    /// checked the same way before it is saved.
    pub(crate) fn audience_errors(
        &self,
        roster: &RosterConfig,
    ) -> Vec<(&'static str, &'static str)> {
        let mut errors = Vec::new();
        if let Some(key) = self.scope_problem() {
            errors.push(("audience", key));
        }
        let audience = self.audience();
        for (key, values) in &audience.attributes {
            let Some(spec) = roster.attributes.get(key) else {
                errors.push(("audience", "announcements.form.bad_audience"));
                continue;
            };
            let off_list =
                spec.kind == AttributeType::Enum && values.iter().any(|v| !spec.values.contains(v));
            if off_list {
                errors.push(("audience", "announcements.form.bad_audience"));
            }
        }
        // #156: a range on a numeric attribute, with bounds that are
        // numbers, the lower not above the upper.
        let before = errors.len();
        for (key, range) in &audience.ranges {
            let numeric = roster
                .attributes
                .get(key)
                .is_some_and(|s| s.kind.is_numeric());
            if !numeric {
                errors.push(("audience", "announcements.form.bad_audience"));
            } else if let Err(e) = range.bounds() {
                let why = match e {
                    RangeError::Reversed => "announcements.form.range_reversed",
                    _ => "announcements.form.range_not_number",
                };
                errors.push(("audience", why));
            }
        }
        let ranged = errors.len() > before;
        if audience.students.len() > MAX_STUDENTS {
            errors.push(("audience", "announcements.form.students_too_many"));
        } else if !ranged && audience.check(roster).is_err() {
            errors.push(("audience", "announcements.form.bad_audience"));
        }
        unique(errors)
    }
}

/// Whether `a` reaches every linked Student: no attribute, Tag or list.
pub(crate) fn is_everyone(a: &Audience) -> bool {
    !a.has_groups() && a.students.is_empty()
}

/// The choice a stored Audience reads as on its edit form. An Audience
/// saved before #120 with groups AND a list matches no single choice:
/// none (`""`), which keeps both filters when saved again.
pub(crate) fn scope_of(a: &Audience) -> &'static str {
    match (a.has_groups(), a.students.is_empty()) {
        (false, true) => "everyone",
        (false, false) => "students",
        (true, true) => "groups",
        (true, false) => "",
    }
}
