//! The Student's side of the walk: chat events on the Fake Channel and
//! what came back through `/dev/outbox`, the OTP from `/dev/email`, the
//! acknowledgement, the Mini App and the self-unlink.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use harpseal::{ActionId, Inbound};
use harpseal_channel_fake::FakeChannel;
use serde_json::Value;

use crate::client::{R, urlencode};
use crate::walk::{CODE, EMAIL, STUDENT, World, actions, ensure, texts};

impl World {
    /// Messages the Student got since the last call, and the Menu.
    async fn news(&self) -> R<(Vec<Value>, Value)> {
        let out = self.c.outbox(STUDENT).await?;
        let all = out["messages"].as_array().cloned().unwrap_or_default();
        let mut seen = self.seen.lock().unwrap_or_else(|p| p.into_inner());
        let new = all.get(*seen..).unwrap_or_default().to_vec();
        *seen = all.len();
        Ok((new, out["menu"].clone()))
    }

    /// Delivers `ev` with a new platform event id, as LINE does, and waits
    /// until its claim is done: the webhook answers 200 before the event
    /// is handled (#97).
    async fn event(&self, ev: Inbound) -> R<(Vec<Value>, Value)> {
        static NEXT: AtomicUsize = AtomicUsize::new(1);
        let id = format!("e2e-{}", NEXT.fetch_add(1, Ordering::Relaxed));
        self.c
            .webhook(&[ev.with_event_id(Some(id.clone()), false)])
            .await?;
        let done = "SELECT count(*) FROM inbound_event WHERE event_id = ? AND done_at IS NOT NULL";
        let deadline = Instant::now() + Duration::from_secs(10);
        while self.int(done, &[&id])? == 0 {
            ensure(
                Instant::now() < deadline,
                format!("event {id} not handled in 10 s"),
            )?;
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        self.news().await
    }

    async fn press(&self, action: &str) -> R<(Vec<Value>, Value)> {
        let action = ActionId::new(action).map_err(|e| e.to_string())?;
        let ev = Inbound::action(FakeChannel::account(STUDENT), action, None);
        self.event(ev).await
    }

    async fn say(&self, text: &str) -> R<(Vec<Value>, Value)> {
        let ev = Inbound::text(FakeChannel::account(STUDENT), text, None);
        self.event(ev).await
    }

    /// Follow: a greeting and the Consent Card; the Menu is locked.
    pub async fn follow(&self) -> R<String> {
        let ev = Inbound::follow(FakeChannel::account(STUDENT), None);
        let (ms, menu) = self.event(ev).await?;
        let card = ms
            .iter()
            .any(|m| actions(m).iter().any(|a| a == "kernel.consent.accept"));
        ensure(card, format!("no Consent Card in: {}", texts(&ms)))?;
        ensure(menu == "locked", format!("Menu {menu}"))?;
        Ok(format!("{} messages, Menu locked", ms.len()))
    }

    /// Accepting asks for the student id.
    pub async fn consent(&self) -> R<String> {
        let (ms, _) = self.press("kernel.consent.accept").await?;
        ensure(!ms.is_empty(), "no answer to accept")?;
        Ok(texts(&ms))
    }

    /// The student id sends an OTP email; returns the code.
    pub async fn student_id(&self) -> R<String> {
        let (ms, _) = self.say(CODE).await?;
        let email = self.c.last_email(EMAIL).await?;
        let subject = email["subject"].as_str().unwrap_or_default();
        let code = subject.rsplit(' ').next().unwrap_or_default().to_string();
        ensure(
            code.len() == 6 && code.bytes().all(|b| b.is_ascii_digit()),
            format!("subject {subject:?}"),
        )?;
        ensure(!ms.is_empty(), "no chat answer to the student id")?;
        Ok(code)
    }

    /// The code links the account and unlocks the Menu.
    pub async fn otp(&self, code: &str) -> R<String> {
        let (ms, menu) = self.say(code).await?;
        let sql = "SELECT count(*) FROM link l JOIN channel_account a ON a.id = l.channel_account_id \
                   WHERE a.external_id = ? AND l.status = 'active'";
        ensure(
            self.int(sql, &[STUDENT])? == 1,
            format!("no active Link; chat: {}", texts(&ms)),
        )?;
        let entries: Vec<String> = menu
            .as_array()
            .ok_or(format!("Menu still {menu}"))?
            .iter()
            .filter_map(|e| e["action"].as_str().map(str::to_string))
            .collect();
        ensure(
            entries.iter().any(|a| a == "kernel.link.unlink"),
            format!("Menu {entries:?}"),
        )?;
        Ok(format!("Link active, Menu {entries:?}"))
    }

    fn ack_action(&self) -> String {
        let id = self
            .announcement
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .clone();
        format!("announcements.ack.{id}")
    }

    /// Ticks until the Card with the ack button arrives.
    pub async fn delivered(&self) -> R<String> {
        let ack = self.ack_action();
        for n in 1..=10 {
            self.c.tick().await?;
            let (ms, _) = self.news().await?;
            if ms.iter().any(|m| actions(m).contains(&ack)) {
                return Ok(format!("Card after {n} tick(s)"));
            }
        }
        Err("no Card with the ack button after 10 ticks".into())
    }

    /// The ack button thanks once, then says "รับทราบแล้ว".
    pub async fn ack(&self) -> R<String> {
        let (first, _) = self.press(&self.ack_action()).await?;
        let first = texts(&first);
        ensure(
            first.contains("บันทึกว่ารับทราบแล้ว"),
            format!("first ack: {first}"),
        )?;
        let (again, _) = self.press(&self.ack_action()).await?;
        let again = texts(&again);
        ensure(again == "รับทราบแล้ว", format!("second ack: {again}"))?;
        Ok(format!("{first} / {again}"))
    }

    /// The Mini App, opened with the Fake platform's token, shows it acked.
    pub async fn mini_app(&self) -> R<String> {
        let id = self
            .announcement
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .clone();
        let next = format!("/app/announcements/{id}");
        let token = self.c.fake().mini_app_token(STUDENT);
        let form = format!(
            "channel=fake&token={}&next={}",
            urlencode(&token),
            urlencode(&next)
        );
        let to = self
            .c
            .post_raw("/app/session", &form)
            .await?
            .redirect("App Session")?;
        ensure(to == next, format!("App Session went to {to}"))?;
        let page = self.c.navigate(&next).await?.expect(200, "Mini App page")?;
        ensure(
            page.body.contains("class=\"app-acked\""),
            "no acknowledged line",
        )?;
        ensure(
            !page.body.contains("class=\"app-ack\""),
            "still offers the ack button",
        )?;
        Ok(format!("{next}: acknowledged"))
    }

    /// Unlink from the Menu: the Link is revoked, the Menu locked.
    pub async fn unlink(&self) -> R<String> {
        let (ms, _) = self.press("kernel.link.unlink").await?;
        let asks = ms
            .iter()
            .any(|m| actions(m).iter().any(|a| a == "kernel.link.unlink_confirm"));
        ensure(asks, format!("no confirmation: {}", texts(&ms)))?;
        let (_, menu) = self.press("kernel.link.unlink_confirm").await?;
        let sql = "SELECT count(*) FROM link l JOIN channel_account a ON a.id = l.channel_account_id \
                   WHERE a.external_id = ? AND l.status = 'revoked'";
        ensure(self.int(sql, &[STUDENT])? == 1, "the Link is not revoked")?;
        ensure(menu == "locked", format!("Menu {menu}"))?;
        Ok("Link revoked, Menu locked".into())
    }
}
