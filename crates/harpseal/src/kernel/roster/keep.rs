//! The fields a row keeps because its sheet leaves their column out
//! (#154): [`Kept`], and [`resolve`], which fills them from the stored
//! Student so a column left out changes nothing.

use std::borrow::Cow;

use super::store::Existing;
use super::validate::Record;

/// A field whose column the row's sheet leaves out.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum Kept {
    Email,
    Locale,
    /// The roster Tags.
    Tags,
    /// An optional attribute, by key.
    Attribute(String),
}

impl Kept {
    /// The field's name as the stored rows write it: `email`, `locale`,
    /// `tags` or `attributes.<key>` (the names `Merge` and the change
    /// list use).
    pub(crate) fn name(&self) -> String {
        match self {
            Self::Email => "email".into(),
            Self::Locale => "locale".into(),
            Self::Tags => "tags".into(),
            Self::Attribute(k) => format!("attributes.{k}"),
        }
    }

    /// The field named `s` ([`Kept::name`]); `None` for any other name.
    pub(crate) fn parse(s: &str) -> Option<Self> {
        match s {
            "email" => Some(Self::Email),
            "locale" => Some(Self::Locale),
            "tags" => Some(Self::Tags),
            _ => s
                .strip_prefix("attributes.")
                .filter(|k| !k.is_empty())
                .map(|k| Self::Attribute(k.to_string())),
        }
    }
}

/// `rec` with the fields its sheet leaves out ([`Record::keep`]) as
/// `existing` stores them: a column left out changes nothing. A new
/// Student keeps the record's empty (or default) values.
pub(crate) fn resolve<'a>(existing: Option<&Existing>, rec: &'a Record) -> Cow<'a, Record> {
    let Some(e) = existing.filter(|_| !rec.keep.is_empty()) else {
        return Cow::Borrowed(rec);
    };
    let mut r = rec.clone();
    for f in &rec.keep {
        match f {
            Kept::Email => r.fields.email = e.fields.email.clone(),
            Kept::Locale => r.fields.locale = e.fields.locale.clone(),
            Kept::Tags => r.tags = e.roster_tags.clone(),
            Kept::Attribute(key) => {
                match e.fields.attributes.get(key) {
                    Some(v) => r.fields.attributes.insert(key.clone(), v.clone()),
                    None => r.fields.attributes.remove(key),
                };
            }
        }
    }
    Cow::Owned(r)
}

#[cfg(test)]
mod tests {
    use super::Kept;

    #[test]
    fn a_name_reads_back_and_a_stranger_does_not() {
        for k in [
            Kept::Email,
            Kept::Locale,
            Kept::Tags,
            Kept::Attribute("gpa".into()),
        ] {
            assert_eq!(Kept::parse(&k.name()), Some(k));
        }
        assert_eq!(Kept::parse("status"), None);
        assert_eq!(Kept::parse("attributes."), None);
    }
}
