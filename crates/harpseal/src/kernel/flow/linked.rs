//! After Link: welcome back from unreachable, re-consent, Module Actions,
//! self-unlink, and free text / files (stored, one fallback per cooldown).
//!
//! Files go to the FileStore under
//! `inbound/<channel>/<channel_account.id>/<inbound_message.id>`, at once,
//! when no larger than `[inbound] max_file_mb`.

use super::account::{self, LinkRow};
use super::{Input, Turn, actions, consent};
use crate::Result;
use crate::delivery::StudentId;
use crate::message::{ActionId, Choices, Message};
use crate::module::ActionContext;
use crate::port_types::Statement;

pub(super) async fn handle(t: &mut Turn<'_>, link: LinkRow, input: Input) -> Result<()> {
    t.texts = t.texts.in_locale(&link.locale);
    let mut welcomed = false;
    if link.unreachable {
        let now = t.s().now_ms();
        let back = Statement::new(
            "UPDATE link SET status = 'active', updated_at = ? WHERE id = ? AND status = 'unreachable'",
        )
        .bind(now)
        .bind(link.id.clone());
        t.s().db.execute(back).await?;
        t.set_menu(Some(&link.locale)).await?;
        t.say("kernel.flow.welcome_back");
        welcomed = true;
    }
    // Re-consent: the account is parked on the Card, or the Student
    // accepted an older notice version than the current one. A Link with
    // no Consent at all and no parked state (seeded data) is left alone;
    // a Manual Link sets `awaiting_reconsent` to ask for it.
    if t.acct.state == "awaiting_reconsent" || (link.any_consent && !link.current_consent) {
        return consent::reconsent(t, link, input).await;
    }
    match input {
        Input::Start => {
            if !welcomed {
                t.say("kernel.flow.welcome_back");
                t.set_menu(Some(&link.locale)).await?;
            }
            t.menu_choices();
            Ok(())
        }
        Input::Action(a) => act(t, &link, a).await,
        other => {
            store(t, &link, &other).await?;
            fallback(t).await
        }
    }
}

/// An Action from a linked, consenting Student.
pub(super) async fn act(t: &mut Turn<'_>, link: &LinkRow, action: ActionId) -> Result<()> {
    let self_unlink = t.d.config().student.self_unlink;
    match action.as_str() {
        actions::UNLINK if self_unlink => {
            let text = t.t("kernel.link.unlink_confirm");
            let options = vec![
                t.action("kernel.link.unlink_yes", actions::UNLINK_CONFIRM)?,
                t.action("kernel.link.unlink_no", actions::UNLINK_KEEP)?,
            ];
            t.out.push(Message::Choices(Choices { text, options }));
            Ok(())
        }
        actions::UNLINK_CONFIRM if self_unlink => unlink(t, link).await,
        actions::UNLINK_KEEP | actions::CONSENT_ACCEPT => {
            t.menu_choices();
            Ok(())
        }
        _ => match t.d.module_action(&action) {
            Some((module, handler)) => {
                let cx = ActionContext {
                    services: t.s().clone(),
                    catalog: t.d.catalog_arc(),
                    student: StudentId(link.student.clone()),
                    account: t.from.clone(),
                    locale: link.locale.clone(),
                };
                let msgs = handler.handle(cx).await?;
                t.module = module;
                t.out.extend(msgs);
                Ok(())
            }
            None => {
                t.say("kernel.action.expired");
                Ok(())
            }
        },
    }
}

/// Revokes the Link (Consent kept), locks the Menu, says goodbye; the
/// account waits for a student id again.
async fn unlink(t: &mut Turn<'_>, link: &LinkRow) -> Result<()> {
    let s = t.s().clone();
    let now = s.now_ms();
    t.acct.flow = account::FlowData {
        consent_version: Some(t.version.clone()),
        ..Default::default()
    };
    let audit = account::audit(
        &s,
        "link.revoke",
        &link.student,
        &link.id,
        &[("status", "revoked".into()), ("by", "student".into())],
    )?;
    let stmts = vec![
        Statement::new(
            "UPDATE link SET status = 'revoked', revoked_at = ?, updated_at = ? WHERE id = ?",
        )
        .bind(now)
        .bind(now)
        .bind(link.id.clone()),
        t.save("awaiting_student_id"),
        audit,
    ];
    s.db.batch(stmts).await?;
    t.set_menu(None).await?;
    t.say("kernel.link.revoked_bye");
    Ok(())
}

/// Stores free text or a file as an `inbound_message` row. A file is
/// copied into the FileStore first; one over the size limit is recorded
/// without bytes and the Student is told.
pub(super) async fn store(t: &mut Turn<'_>, link: &LinkRow, input: &Input) -> Result<()> {
    let s = t.s().clone();
    let id = s.new_id()?;
    let insert = Statement::new(
        "INSERT INTO inbound_message (id, channel_account_id, student_id, kind, text, file_key, \
         file_name, content_type, size_bytes, received_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(id.clone())
    .bind(t.acct.id.clone())
    .bind(link.student.clone());
    let stmt = match input {
        Input::Text(text) => insert
            .bind("text")
            .bind(text.clone())
            .bind(None::<String>)
            .bind(None::<String>)
            .bind(None::<String>)
            .bind(None::<i64>),
        Input::File(file) => {
            let mb = t.d.config().inbound.max_file_mb;
            let max = u64::from(mb) * 1024 * 1024;
            let mut bytes = None;
            if file.size_bytes.is_none_or(|n| n <= max) {
                let got =
                    t.d.channel_or_err(t.from.channel.as_str())?
                        .download(file)
                        .await?;
                bytes = (got.len() as u64 <= max).then_some(got);
            }
            let content_type = file
                .content_type
                .clone()
                .unwrap_or_else(|| "application/octet-stream".into());
            let size = bytes.as_ref().map(|b| b.len() as u64).or(file.size_bytes);
            let key = match bytes {
                Some(b) => {
                    let key = format!("inbound/{}/{}/{id}", t.from.channel.as_str(), t.acct.id);
                    crate::check_file_key(&key)?;
                    s.files.put(&key, b, &content_type).await?;
                    Some(key)
                }
                None => {
                    let text = t.tf("kernel.inbound.file_too_large", &[("mb", &mb.to_string())]);
                    t.out.push(Message::text(text));
                    None
                }
            };
            insert
                .bind("file")
                .bind(None::<String>)
                .bind(key)
                .bind(file.file_name.clone())
                .bind(content_type)
                .bind(size.and_then(|n| i64::try_from(n).ok()))
        }
        _ => return Ok(()),
    };
    s.db.execute(stmt.bind(s.now_ms())).await?;
    Ok(())
}

/// The fallback Text + Menu Choices, at most once per cooldown per account.
async fn fallback(t: &mut Turn<'_>) -> Result<()> {
    let now = t.s().now_ms();
    let cooldown = i64::from(t.d.config().inbound.fallback_cooldown_minutes) * 60_000;
    if t.acct
        .flow
        .last_fallback_at
        .is_some_and(|at| now - at < cooldown)
    {
        return Ok(());
    }
    t.acct.flow.last_fallback_at = Some(now);
    let state = if t.acct.state == "awaiting_reconsent" {
        "awaiting_reconsent"
    } else {
        "linked"
    };
    t.s().db.execute(t.save(state)).await?;
    t.say("kernel.inbound.fallback");
    t.menu_choices();
    Ok(())
}
