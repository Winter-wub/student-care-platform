//! The busy state (#131): a submit that takes a moment shows that it is
//! working and is not sent twice by a double click. No client library:
//! HTMX's own `htmx-request` class, `aria-busy`, and this small inline
//! script on every back-office page.
//!
//! - A plain form, once it submits, gets `aria-busy="true"` and the time
//!   (`data-busy-at`); its submit buttons that were not already
//!   `aria-disabled` get it (and `data-busy-held`), and the pressed one
//!   `data-busy` (drawn with a ring in `ui_states.css`). A second submit
//!   while it is busy is cancelled (`preventDefault`). Nothing is
//!   `disabled`, so the pressed button's `name`/`value` (`op=preview` on
//!   the branding page) always reaches the server.
//! - After 30 s (`HOLD` in the script) with no new page (Stop pressed, a response that
//!   never navigates) the form lets go by itself: the ring goes and the
//!   next submit is sent.
//! - A page restored from the back/forward cache (`pageshow` with
//!   `persisted`) clears whatever the submit left; a fresh load has none.
//! - An HTMX request sent by a form marks that form from `htmx:beforeSend`
//!   until `htmx:afterRequest`; HTMX queues repeats itself, so none is
//!   cancelled here. Its target is never marked: targets are live
//!   regions (`#pick-results`) or polled (the roster progress).
//! - A form with `data-no-busy` is left alone. Put it on a form whose
//!   submit answers a download or `204 No Content`: no page follows, so
//!   it would read busy until 30 s (`HOLD` in the script).

/// The script, inline in the back-office document (no build step).
pub(crate) const BUSY_JS: &str = r#"(function(){
var Q='button[type=submit],button:not([type]),input[type=submit]',HOLD=30000;
function busy(f){return f.getAttribute('aria-busy')==='true'&&Date.now()-(+f.getAttribute('data-busy-at')||0)<HOLD;}
function release(f){f.removeAttribute('aria-busy');f.removeAttribute('data-busy-at');
f.querySelectorAll('[data-busy]').forEach(function(b){b.removeAttribute('data-busy');});
f.querySelectorAll('[data-busy-held]').forEach(function(b){b.removeAttribute('aria-disabled');b.removeAttribute('data-busy-held');});}
function hold(f,by){release(f);var at=String(Date.now());
f.setAttribute('aria-busy','true');f.setAttribute('data-busy-at',at);
f.querySelectorAll(Q).forEach(function(b){if(b.getAttribute('aria-disabled')!=='true'){b.setAttribute('aria-disabled','true');b.setAttribute('data-busy-held','');}});
if(by&&by.form===f)by.setAttribute('data-busy','');
setTimeout(function(){if(f.getAttribute('data-busy-at')===at)release(f);},HOLD);}
function own(f){return f&&f.tagName==='FORM'&&!f.hasAttribute('data-no-busy');}
document.addEventListener('submit',function(e){var f=e.target;
if(!own(f)||f.hasAttribute('hx-post')||f.hasAttribute('hx-get'))return;
if(busy(f)){e.preventDefault();return;}
if(!e.defaultPrevented)hold(f,e.submitter);});
addEventListener('pageshow',function(e){if(e.persisted)document.querySelectorAll('form[aria-busy]').forEach(release);});
document.addEventListener('htmx:beforeSend',function(e){var f=e.detail.elt;if(own(f))hold(f,null);});
document.addEventListener('htmx:afterRequest',function(e){var f=e.detail.elt;if(own(f))release(f);});
})();"#;

#[cfg(test)]
mod tests {
    use super::BUSY_JS;

    #[test]
    fn the_script_blocks_repeats_for_a_while_without_disabling_anything() {
        // A disabled submitter would drop its name/value from the post.
        assert!(!BUSY_JS.contains("disabled=true") && !BUSY_JS.contains(".disabled"));
        assert!(BUSY_JS.contains("if(busy(f)){e.preventDefault();return;}"));
        // The hold has a time and lets go by itself after 30 s.
        assert!(BUSY_JS.contains("HOLD=30000"));
        assert!(BUSY_JS.contains("Date.now()-(+f.getAttribute('data-busy-at')||0)<HOLD"));
        assert!(BUSY_JS.contains("},HOLD);"));
        // Only a page from the back/forward cache needs clearing.
        assert!(BUSY_JS.contains("if(e.persisted)"));
        // HTMX queues repeats itself: its path cancels nothing, and its
        // target (a live region, a polled block) is never marked.
        let htmx = &BUSY_JS[BUSY_JS.find("htmx:beforeSend").unwrap()..];
        assert!(!htmx.contains("preventDefault"));
        assert!(!BUSY_JS.contains("detail.target"));
        // Release lets go only of what the hold marked.
        assert!(BUSY_JS.contains("querySelectorAll('[data-busy-held]')"));
        // Nothing that could close the inline `<script>` early.
        assert!(!BUSY_JS.contains("</"));
    }
}
