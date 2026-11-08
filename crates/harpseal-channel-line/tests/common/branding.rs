//! A Deployment with a locked Menu for the branding tests (#114).

#![allow(dead_code)] // each test binary uses part of this

use std::sync::Arc;

use harpseal::{
    ActionId, ChannelAccountId, Deployment, DeploymentConfig, HttpClient, Menu, MenuEntry,
    Statement,
};
use harpseal_channel_line::{KIND, LineChannel, LineConfig};
use harpseal_testing::Harness;

use super::relink::{CONFIG, user};
use super::{SECRET, TOKEN, png};

/// A Deployment on `h` sending through `http`, with a locked Menu,
/// `linked` linked accounts (`ca00000`..) and `locked` that are not linked
/// after them (all shown the configured locked image), and an Editor.
pub async fn with_locked(
    h: Harness,
    http: Arc<dyn HttpClient>,
    linked: usize,
    locked: usize,
) -> (Harness, Deployment) {
    let start = Menu::new(vec![MenuEntry {
        label: "เริ่มใช้งาน".into(),
        action: ActionId::new("kernel.link.start").unwrap(),
    }]);
    let cfg = LineConfig::new(SECRET, TOKEN, "")
        .with_menu_images(png(2500, 843), png(2500, 843))
        .with_locked_menu(start);
    let line = LineChannel::new(cfg, http);
    let config = DeploymentConfig::from_toml(CONFIG).unwrap();
    let d = h.builder().config(config).channel(line).build().unwrap();
    d.migrate().await.unwrap();
    for i in 0..linked + locked {
        let state = if i < linked { "linked" } else { "new" };
        let ca = format!(
            "INSERT INTO channel_account (id, channel, external_id, first_seen_at, flow_state, \
             updated_at) VALUES ('ca{i:05}', 'line', '{}', 1, '{state}', 1)",
            user(i)
        );
        h.db().execute(Statement::new(ca)).await.unwrap();
    }
    let ch = d.channel("line").unwrap();
    for i in linked..linked + locked {
        ch.set_menu(&ChannelAccountId::new(KIND, user(i)), None)
            .await
            .unwrap();
    }
    h.create_staff("editor@sealbay.example", "kelp forest at dawn", &["Editor"])
        .await;
    (h, d)
}
