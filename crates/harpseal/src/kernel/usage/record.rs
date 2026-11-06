//! The notification record of Job `kernel.usage.watch`: a `setting` row
//! `kernel.usage.notified.<kind>` (Locale `''`, never shown on the Texts
//! page) holding
//! `{"month":"2026-11","level":"warn","pending":[{"email":"…","tries":1}],"sent":2,"dropped":0}`.

use serde_json::{Value as Json, json};

use super::quota::Level;

/// Sends to one address per claim: the first, then two retries.
pub(super) const TRIES: u32 = 3;

/// An address the last send could not reach, and how often it was tried.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Pending {
    pub(super) email: String,
    pub(super) tries: u32,
}

/// What a `setting` record says.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Record {
    pub(super) month: String,
    pub(super) level: Level,
    /// Addresses to retry on the next run.
    pub(super) pending: Vec<Pending>,
    /// Emails that went out for this claim so far.
    pub(super) sent: u64,
    /// Addresses given up on after [`TRIES`] failed sends.
    pub(super) dropped: u64,
}

impl Record {
    /// A fresh claim of `level` in `month`: nothing sent yet.
    pub(super) fn claim(month: &str, level: Level) -> Self {
        Self {
            month: month.to_string(),
            level,
            pending: Vec::new(),
            sent: 0,
            dropped: 0,
        }
    }

    pub(super) fn parse(v: &str) -> Option<Self> {
        let j: Json = serde_json::from_str(v).ok()?;
        let level = match j.get("level")?.as_str()? {
            "warn" => Level::Warn,
            "over" => Level::Over,
            _ => Level::Ok,
        };
        let count = |k: &str| j.get(k).and_then(Json::as_u64).unwrap_or(0);
        let pending = j
            .get("pending")
            .and_then(Json::as_array)
            .map(|a| {
                a.iter()
                    .filter_map(|e| {
                        Some(Pending {
                            email: e.get("email")?.as_str()?.to_string(),
                            tries: e.get("tries").and_then(Json::as_u64).unwrap_or(1) as u32,
                        })
                    })
                    .collect()
            })
            .unwrap_or_default();
        Some(Self {
            month: j.get("month")?.as_str()?.to_string(),
            level,
            pending,
            sent: count("sent"),
            dropped: count("dropped"),
        })
    }

    pub(super) fn to_value(&self) -> String {
        let pending: Vec<Json> = self
            .pending
            .iter()
            .map(|p| json!({ "email": p.email, "tries": p.tries }))
            .collect();
        json!({
            "month": self.month, "level": self.level.as_str(), "pending": pending,
            "sent": self.sent, "dropped": self.dropped,
        })
        .to_string()
    }

    /// The record after a send to addresses tried `before` times each (0
    /// for the claim's first send): `sent` went out; each of `failed`
    /// stays pending until its [`TRIES`]th failure, then is dropped.
    pub(super) fn after_send(&self, sent: usize, failed: &[Pending]) -> Self {
        let mut next = Self {
            pending: Vec::new(),
            sent: self.sent + sent as u64,
            ..self.clone()
        };
        for p in failed {
            let tries = p.tries + 1;
            if tries >= TRIES {
                next.dropped += 1;
            } else {
                next.pending.push(Pending {
                    email: p.email.clone(),
                    tries,
                });
            }
        }
        next
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(email: &str, tries: u32) -> Pending {
        Pending {
            email: email.into(),
            tries,
        }
    }

    #[test]
    fn round_trips() {
        let mut r = Record::claim("2026-11", Level::Warn);
        r.pending = vec![p("a@sealbay.example", 2)];
        r.sent = 3;
        r.dropped = 1;
        assert_eq!(Record::parse(&r.to_value()), Some(r));
    }

    #[test]
    fn an_address_is_dropped_on_its_third_failure() {
        let r = Record::claim("2026-11", Level::Over);
        let r = r.after_send(1, &[p("a@x", 0)]);
        assert_eq!(
            (r.sent, r.dropped, r.pending.clone()),
            (1, 0, vec![p("a@x", 1)])
        );
        let r = r.after_send(0, &r.pending.clone());
        assert_eq!(r.pending, vec![p("a@x", 2)]);
        let r = r.after_send(0, &r.pending.clone());
        assert_eq!((r.sent, r.dropped, r.pending.is_empty()), (1, 1, true));
    }
}
