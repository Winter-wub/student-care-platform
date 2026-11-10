//! The Manual Link picker's markup (#143): the area that loads the list on
//! request, the list itself, and the typed id behind a disclosure.

use maud::{Markup, PreEscaped, html};

use super::pick::{EVENT_WINDOW_HOURS, PICK, Picker, RECENT_CAP, Recent, short_id};
use super::view::{View, channel_label};
use crate::ChannelKind;

/// Two accounts that wrote within this long of the list being read may be
/// confused: the list warns before Staff pick.
const BUSY_MINUTES: i64 = 2;
const BUSY_MS: i64 = BUSY_MINUTES * 60_000;

/// The flow stage's catalog key. A state an unlinked account should not
/// be in (`linked` after a revoke, an unknown one) gets a neutral text,
/// never "not started".
fn stage_key(state: &str) -> &'static str {
    match state {
        "new" => "kernel.students.pick_stage_new",
        "awaiting_consent" | "awaiting_reconsent" => "kernel.students.pick_stage_consent",
        "awaiting_student_id" => "kernel.students.pick_stage_student_id",
        "awaiting_otp" => "kernel.students.pick_stage_otp",
        _ => "kernel.students.pick_stage_other",
    }
}

/// `ms` as "45 วินาทีที่แล้ว", "2 นาทีที่แล้ว" or "3 ชั่วโมงที่แล้ว" from
/// `now`; `None` from a day on (the date says it).
fn ago(v: &View<'_>, now: i64, ms: i64) -> Option<String> {
    let secs = (now - ms).max(0) / 1000;
    let (key, n) = match secs {
        0..60 => ("kernel.students.pick_ago_seconds", secs),
        60..3600 => ("kernel.students.pick_ago_minutes", secs / 60),
        3600..86_400 => ("kernel.students.pick_ago_hours", secs / 3600),
        _ => return None,
    };
    Some(v.tf(key, &[("n", &n.to_string())]))
}

/// How many listed accounts wrote within [`BUSY_MS`] of `now`.
fn busy(p: &Picker) -> usize {
    p.recent
        .iter()
        .filter(|r| p.now - r.last_seen <= BUSY_MS)
        .count()
}

/// The picker's area in the Manual Link form: the list when `picker` was
/// read (`?pick=1`), else the hint and the control that loads it.
pub(super) fn area(v: &View<'_>, base: &str, picker: Option<&Picker>) -> Markup {
    html! {
        div id="ml-pick" class="pick-area" {
            @match picker {
                Some(p) => (list(v, base, p)),
                None => {
                    p class="muted" { (v.t("kernel.students.pick_hint")) }
                    (load_link(v, base, "kernel.students.pick_load"))
                }
            }
        }
    }
}

/// The control that (re)loads the list: an HTMX GET into `#ml-pick`, or
/// without script the Student's page with the list open.
fn load_link(v: &View<'_>, base: &str, key: &str) -> Markup {
    html! {
        a class="btn secondary pick-load" href=(format!("{base}?pick=1#ml-pick"))
            hx-get=(format!("{base}/links/recent")) hx-target="#ml-pick" hx-swap="innerHTML" {
            (v.t(key))
        }
    }
}

/// The list of recent unlinked accounts as radios (none chosen), with a
/// warning when more than one wrote in the last [`BUSY_MINUTES`].
pub(super) fn list(v: &View<'_>, base: &str, p: &Picker) -> Markup {
    html! {
        fieldset class="checks pick" tabindex="-1" aria-describedby="ml-pick-hint" {
            legend { (v.t("kernel.students.pick_title")) }
            p id="ml-pick-hint" class="muted" { (v.t("kernel.students.pick_check")) }
            @if busy(p) > 1 {
                p class="note warn-note" { (v.tf("kernel.students.pick_busy", &[("minutes", &BUSY_MINUTES.to_string())])) }
            }
            @if p.recent.is_empty() {
                p class="muted" { (v.tf("kernel.students.pick_none", &[("hours", &EVENT_WINDOW_HOURS.to_string())])) }
            } @else {
                ul class="list" {
                    @for (i, r) in p.recent.iter().enumerate() { (row(v, p.now, i, r)) }
                }
                @if p.more {
                    p class="muted" {
                        (v.tf("kernel.students.pick_capped", &[("count", &RECENT_CAP.to_string())]))
                    }
                }
            }
        }
        (load_link(v, base, "kernel.students.pick_reload"))
    }
}

fn row(v: &View<'_>, now: i64, i: usize, r: &Recent) -> Markup {
    html! {
        li {
            label class="choice" for=(format!("ml-pick-{i}")) {
                input id=(format!("ml-pick-{i}")) type="radio" name=(PICK)
                    value=(format!("{}:{}", r.channel, r.external_id));
                span {
                    span class="name" { (channel_label(&r.channel)) " " }
                    span class="code" { (short_id(&r.external_id)) }
                    br;
                    span class="muted" {
                        (v.t("kernel.students.pick_last")) " "
                        @if let Some(a) = ago(v, now, r.last_seen) { strong { (a) } " · " }
                        (v.time(r.last_seen)) " · " (v.t(stage_key(&r.state)))
                    }
                    @if r.blocked {
                        br;
                        span class="field-warning" { (v.t("kernel.students.pick_blocked")) }
                    }
                    br;
                    span class="muted" { (v.t("kernel.students.pick_first")) " " (v.time(r.first_seen)) }
                }
            }
        }
    }
}

/// The raw Channel and id, closed, for support cases; and the script that
/// keeps a pick and a typed id from being posted together.
pub(super) fn typed(v: &View<'_>, offered: &[ChannelKind]) -> Markup {
    html! {
        details class="pick-typed" {
            summary { (v.t("kernel.students.pick_typed")) }
            div class="row2" {
                div class="field" {
                    label for="ml-channel" { (v.t("kernel.students.channel")) }
                    select id="ml-channel" name="channel" {
                        @for kind in offered { option value=(kind.as_str()) { (channel_label(kind.as_str())) } }
                    }
                }
                div class="field" {
                    label for="ml-external" { (v.t("kernel.students.external_id")) }
                    input id="ml-external" name="external_id" type="text" autocomplete="off";
                }
            }
        }
        script { (PreEscaped(PICK_JS)) }
    }
}

/// Picking an account clears the typed id, and typing one clears the
/// pick, so the form never posts two different accounts (without script
/// the server refuses that instead of guessing). A loaded list takes the
/// focus, so its legend is read and the keyboard is where the list is.
/// No library, no build step.
const PICK_JS: &str = r#"(function(){
var t=document.getElementById('ml-external');if(!t)return;var f=t.form;
f.addEventListener('change',function(e){if(e.target.name==='pick'&&e.target.checked)t.value='';});
t.addEventListener('input',function(){if(t.value)f.querySelectorAll('input[name=pick]').forEach(function(r){r.checked=false;});});
document.addEventListener('htmx:afterSwap',function(e){if(e.detail.target&&e.detail.target.id==='ml-pick'){var l=e.detail.target.querySelector('fieldset');if(l)l.focus();}});
})();"#;
