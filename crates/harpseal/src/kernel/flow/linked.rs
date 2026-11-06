//! After Link: welcome back from unreachable, re-consent, Module Actions,
//! self-unlink, and free text / files (stored, one fallback per cooldown).
//!
//! Files go to the FileStore under
//! `inbound/<channel>/<channel_account.id>/<inbound_message.id>`, at once,
//! when no larger than `[inbound] max_file_mb`. A file that could not be
//! copied (too large, download or FileStore error) still gets its row,
//! with `file_key` NULL and the reason in `file_error`.

use super::account::{self, LinkRow};
use super::{Input, Turn, actions, consent};
use crate::Result;
use crate::audit::{Actor, RequestCtx};
use crate::delivery::{IncomingFile, StudentId};
use crate::kernel::links;
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
        // Of racing Inbounds, only the one that restored it says so.
        if t.s().db.execute(back).await?.rows_affected == 1 {
            t.set_menu(Some(&link.locale)).await?;
            t.say("kernel.flow.welcome_back");
        }
        welcomed = true;
    }
    // Re-consent: no Consent for the current version, and the account is
    // parked on the Card or the Student accepted an older version. A Link
    // with no Consent at all and no parked state (seeded data) is left
    // alone; a Manual Link sets `awaiting_reconsent` to ask for it.
    let parked_on_card = t.acct.state == "awaiting_reconsent";
    if !link.current_consent && (parked_on_card || link.any_consent) {
        return consent::reconsent(t, link, input).await;
    }
    if parked_on_card && !consent::already_accepted(t, &link).await? {
        return Ok(());
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
                    catalog: t.texts.catalog(),
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

/// Revokes the Link (Consent kept) through `kernel::links` with the
/// Student as the actor, locks the Menu, says goodbye; the account waits
/// for a student id again.
async fn unlink(t: &mut Turn<'_>, link: &LinkRow) -> Result<()> {
    let s = t.s().clone();
    t.acct.flow = account::FlowData {
        consent_version: Some(t.version.clone()),
        ..Default::default()
    };
    let actor = Actor::Student(link.student.clone());
    let (stmts, _) =
        links::revoke_statements(&s, &link.id, &link.student, &actor, &RequestCtx::default())?;
    if !t.commit(stmts, "awaiting_student_id").await? {
        return Ok(());
    }
    t.set_menu(None).await?;
    t.say("kernel.link.revoked_bye");
    Ok(())
}

/// What became of a file: its FileStore key, its size, or why not.
struct Copied {
    key: Option<String>,
    size: Option<u64>,
    error: Option<String>,
}

/// Downloads and stores one file. Never fails: the reason goes into
/// `file_error` (cut to 200 characters), and a file over the limit is
/// also told to the Student.
async fn copy(t: &mut Turn<'_>, file: &IncomingFile, id: &str, content_type: &str) -> Copied {
    let mb = t.d.config().inbound.max_file_mb;
    let max = u64::from(mb) * 1024 * 1024;
    let too_large = |t: &mut Turn<'_>, size: Option<u64>| {
        let text = t.tf("kernel.inbound.file_too_large", &[("mb", &mb.to_string())]);
        t.out.push(Message::text(text));
        Copied {
            key: None,
            size,
            error: Some("too_large".into()),
        }
    };
    if file.size_bytes.is_some_and(|n| n > max) {
        return too_large(t, file.size_bytes);
    }
    let failed = |e: crate::Error| Copied {
        key: None,
        size: file.size_bytes,
        error: Some(e.to_string().chars().take(200).collect()),
    };
    let channel = match t.d.channel_or_err(t.from.channel.as_str()) {
        Ok(c) => c,
        Err(e) => return failed(e),
    };
    let bytes = match channel.download(file).await {
        Ok(b) => b,
        Err(e) => return failed(e),
    };
    let size = Some(bytes.len() as u64);
    if bytes.len() as u64 > max {
        return too_large(t, size);
    }
    let key = format!("inbound/{}/{}/{id}", t.from.channel.as_str(), t.acct.id);
    let stored = match crate::check_file_key(&key) {
        Ok(()) => t.s().files.put(&key, bytes, content_type).await,
        Err(e) => Err(e),
    };
    match stored {
        Ok(()) => Copied {
            key: Some(key),
            size,
            error: None,
        },
        Err(e) => Copied { size, ..failed(e) },
    }
}

/// Stores free text or a file as an `inbound_message` row (a file is
/// copied into the FileStore first, see [`copy`]).
pub(super) async fn store(t: &mut Turn<'_>, link: &LinkRow, input: &Input) -> Result<()> {
    let s = t.s().clone();
    let id = s.new_id()?;
    let insert = Statement::new(
        "INSERT INTO inbound_message (id, channel_account_id, student_id, kind, text, file_key, \
         file_name, content_type, size_bytes, file_error, received_at) \
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
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
            .bind(None::<i64>)
            .bind(None::<String>),
        Input::File(file) => {
            let content_type = file
                .content_type
                .clone()
                .unwrap_or_else(|| "application/octet-stream".into());
            let copied = copy(t, file, &id, &content_type).await;
            insert
                .bind("file")
                .bind(None::<String>)
                .bind(copied.key)
                .bind(file.file_name.clone())
                .bind(content_type)
                .bind(copied.size.and_then(|n| i64::try_from(n).ok()))
                .bind(copied.error)
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
    t.say("kernel.inbound.fallback");
    t.menu_choices();
    t.save(state).await?;
    Ok(())
}
