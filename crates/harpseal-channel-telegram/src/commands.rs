//! The command map: Menu commands (`/news_latest`) → Actions, from
//! [`crate::TelegramChannel::with_menu`] and a per-account overlay written
//! by `set_menu`.

use std::collections::HashMap;
use std::sync::Arc;

use harpseal::Menu;

use crate::render;
use crate::update::CommandMap;

/// The command map for `menu` (see [`render::commands`]).
pub fn command_map(menu: &Menu) -> CommandMap {
    render::commands(menu)
        .into_iter()
        .map(|c| (c.command, c.action))
        .collect()
}

#[derive(Default)]
pub(crate) struct Commands {
    /// From [`TelegramChannel::with_menu`].
    pub(crate) base: CommandMap,
    /// From `set_menu`, per chat id. Accounts with the same Menu share one
    /// map, so memory grows by a key and a pointer per account.
    pub(crate) overlay: HashMap<String, Arc<CommandMap>>,
    /// The distinct maps in `overlay` (a handful: locked, and one
    /// unlocked Menu per Locale).
    shared: Vec<Arc<CommandMap>>,
}

impl Commands {
    /// Sets `chat`'s map, reusing an identical one already held.
    pub(crate) fn set(&mut self, chat: String, map: CommandMap) {
        let map = match self.shared.iter().find(|m| ***m == map) {
            Some(m) => m.clone(),
            None => {
                let m = Arc::new(map);
                self.shared.push(m.clone());
                m
            }
        };
        self.overlay.insert(chat, map);
        // Forget maps no account uses any more.
        self.shared.retain(|m| Arc::strong_count(m) > 1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use harpseal::ActionId;

    fn map(action: &str) -> CommandMap {
        let a = ActionId::new(action).unwrap();
        CommandMap::from([(render::command_name(action), a)])
    }

    #[test]
    fn accounts_with_the_same_menu_share_one_map() {
        let mut c = Commands::default();
        for chat in 0..1000 {
            c.set(chat.to_string(), map("news.latest"));
        }
        assert_eq!(c.shared.len(), 1);
        assert!(Arc::ptr_eq(&c.overlay["0"], &c.overlay["999"]));
        c.set("0".into(), map("faq.list"));
        assert_eq!(c.shared.len(), 2);
        for chat in 1..1000 {
            c.set(chat.to_string(), map("faq.list"));
        }
        assert_eq!(c.shared.len(), 1, "a map no account uses is forgotten");
    }
}
