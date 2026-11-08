//! How the Fake Channel "renders": it degrades a Message into the objects a
//! platform with the profile's limits would show, still as `Message`
//! values so tests can read them. Same rules as the real adapters
//! (`docs/prototypes/message-render.md`), never dropping an Action button.

use harpseal::render::{char_len, more_label, split_text};
use harpseal::{Button, Card, CardList, Choices, Locale, Message, RenderCx};

/// The limits of one profile. `usize::MAX` means "no limit".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Limits {
    /// Longest text in chars; longer is split.
    pub text: usize,
    /// Most Cards in one CardList object; `1` sends one message per Card.
    pub cards: usize,
    /// Most quick-reply options; more turn Choices into a Card holding
    /// every button (LINE).
    pub quick_reply: usize,
    /// Longest quick-reply label in chars (LINE: 20).
    pub quick_reply_label: usize,
    /// Most buttons on one message; more go to follow-up messages
    /// (Telegram's inline keyboard).
    pub buttons: usize,
}

impl Limits {
    pub(crate) const NONE: Limits = Limits {
        text: usize::MAX,
        cards: usize::MAX,
        quick_reply: usize::MAX,
        quick_reply_label: usize::MAX,
        buttons: usize::MAX,
    };
    pub(crate) const LINE: Limits = Limits {
        text: 5000,
        cards: 12,
        quick_reply: 13,
        quick_reply_label: 20,
        buttons: usize::MAX,
    };
    pub(crate) const TELEGRAM: Limits = Limits {
        text: 4096,
        cards: 1,
        quick_reply: usize::MAX,
        quick_reply_label: usize::MAX,
        buttons: 100,
    };
}

/// A URL button the Fake refuses, as the real adapters do: one whose URL
/// is not https ([`Button::safe_url`]). Built by hand inside the Core
/// only; [`Button::url`] never makes one.
pub(crate) fn refused_url(msg: &Message) -> bool {
    msg.buttons()
        .iter()
        .any(|b| matches!(b, Button::Url { .. }) && b.safe_url().is_none())
}

/// The error a refused URL button sends back.
pub(crate) fn refused() -> harpseal::Error {
    harpseal::Error::Invalid("a URL button needs an https: URL".into())
}

/// The objects `msg` becomes under `limits`. A URL button is kept as it
/// is (recorded; the platform would show it as a link button).
pub(crate) fn render(msg: &Message, limits: Limits, locale: &Locale) -> Vec<Message> {
    match msg {
        Message::Text(t) => split_text(t, limits.text)
            .into_iter()
            .map(Message::Text)
            .collect(),
        Message::CardList(list) if limits.cards == 1 => {
            list.0.iter().cloned().map(Message::Card).collect()
        }
        Message::CardList(list) => list
            .0
            .chunks(limits.cards)
            .map(|c| Message::CardList(CardList(c.to_vec())))
            .collect(),
        Message::Choices(c) => choices(c, limits, locale),
        other => vec![other.clone()],
    }
}

/// Long text is split with the buttons on the last part; too many options
/// (or a label too long for a quick reply) become one Card with every
/// button; more buttons than one message holds go to follow-ups.
fn choices(c: &Choices, limits: Limits, locale: &Locale) -> Vec<Message> {
    let mut parts = split_text(&c.text, limits.text);
    let last = parts.pop().unwrap_or_default();
    let mut out: Vec<Message> = parts.into_iter().map(Message::Text).collect();
    let too_many = c.options.len() > limits.quick_reply
        || c.options
            .iter()
            .any(|b| char_len(b.label()) > limits.quick_reply_label);
    if too_many {
        out.push(Message::Card(Card {
            title: String::new(),
            body: last,
            buttons: c.options.clone(),
        }));
        return out;
    }
    let chunks: Vec<&[Button]> = c.options.chunks(limits.buttons.max(1)).collect();
    let cx = RenderCx {
        locale: locale.clone(),
        mini_app_base_url: None,
    };
    for (i, chunk) in chunks.iter().enumerate() {
        let text = if i == 0 {
            last.clone()
        } else {
            more_label(&cx, i + 1, chunks.len())
        };
        out.push(Message::Choices(Choices {
            text,
            options: chunk.to_vec(),
        }));
    }
    if chunks.is_empty() {
        out.push(Message::Choices(Choices {
            text: last,
            options: Vec::new(),
        }));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use harpseal::ActionId;

    fn options(n: usize) -> Vec<Button> {
        (0..n)
            .map(|i| Button::action(format!("o{i}"), ActionId::new(format!("a.{i}")).unwrap()))
            .collect()
    }

    fn actions(msgs: &[Message]) -> usize {
        msgs.iter()
            .map(|m| harpseal::render::action_ids(m).len())
            .sum()
    }

    #[test]
    fn line_turns_long_choices_into_one_card() {
        let c = Message::Choices(Choices {
            text: "เลือก".into(),
            options: options(20),
        });
        let out = render(&c, Limits::LINE, &Locale::th());
        assert_eq!(out.len(), 1);
        assert!(matches!(&out[0], Message::Card(_)));
        assert_eq!(actions(&out), 20);
    }

    #[test]
    fn telegram_sends_follow_ups_past_100_buttons() {
        let c = Message::Choices(Choices {
            text: "x".repeat(5000),
            options: options(150),
        });
        let out = render(&c, Limits::TELEGRAM, &Locale::en());
        assert_eq!(out.len(), 3, "text part, 100 buttons, 50 buttons");
        assert!(matches!(&out[0], Message::Text(_)));
        assert_eq!(actions(&out), 150);
    }
}
