//! The busy state (#131): a submit that takes a moment shows that it is
//! working and cannot be pressed twice. No client library: HTMX's own
//! `htmx-request` class, `aria-busy`, and this small inline script on
//! every back-office page.
//!
//! - A plain form, once it submits (after the browser collected its data,
//!   so the pressed button's `name`/`value` still go), gets
//!   `aria-busy="true"`, its submit buttons are disabled, and the pressed
//!   one is marked `data-busy` (drawn with a ring in `ui_states.css`). A
//!   page restored from the back/forward cache, or a response that never
//!   navigates (a download), releases it again.
//! - An HTMX request marks its target `aria-busy="true"` and disables the
//!   submit buttons of the form that sent it, until the request ends.
//! - A form with `data-no-busy` is left alone.

/// The script, inline in the back-office document (no build step).
pub(crate) const BUSY_JS: &str = r#"(function(){
var Q='button[type=submit],button:not([type]),input[type=submit]';
function hold(f,by){f.setAttribute('aria-busy','true');
f.querySelectorAll(Q).forEach(function(b){if(!b.disabled){b.disabled=true;b.setAttribute('data-busy-off','');}});
if(by&&by.form===f)by.setAttribute('data-busy','');}
function release(f){f.removeAttribute('aria-busy');
f.querySelectorAll('[data-busy]').forEach(function(b){b.removeAttribute('data-busy');});
f.querySelectorAll('[data-busy-off]').forEach(function(b){b.disabled=false;b.removeAttribute('data-busy-off');});}
document.addEventListener('submit',function(e){var f=e.target;
if(e.defaultPrevented||f.hasAttribute('data-no-busy')||f.hasAttribute('hx-post')||f.hasAttribute('hx-get'))return;
var by=e.submitter;setTimeout(function(){hold(f,by);setTimeout(function(){release(f);},15000);},0);});
addEventListener('pageshow',function(e){if(e.persisted)document.querySelectorAll('form[aria-busy]').forEach(release);});
document.addEventListener('htmx:beforeRequest',function(e){var d=e.detail;
if(d.target)d.target.setAttribute('aria-busy','true');
if(d.elt&&d.elt.tagName==='FORM'&&!d.elt.hasAttribute('data-no-busy'))hold(d.elt,null);});
document.addEventListener('htmx:afterRequest',function(e){var d=e.detail;
if(d.target)d.target.removeAttribute('aria-busy');
if(d.elt&&d.elt.tagName==='FORM')release(d.elt);});
})();"#;

#[cfg(test)]
mod tests {
    use super::BUSY_JS;

    #[test]
    fn the_script_holds_after_the_data_is_collected_and_lets_go() {
        // Disabled before the browser collects the entry list, the pressed
        // button's name and value would be lost: the hold waits a turn.
        assert!(BUSY_JS.contains("setTimeout(function(){hold(f,by)"));
        assert!(BUSY_JS.contains("aria-busy"));
        assert!(BUSY_JS.contains("pageshow"));
        assert!(BUSY_JS.contains("htmx:afterRequest"));
        // Nothing that could close the inline `<script>` early.
        assert!(!BUSY_JS.contains("</"));
    }
}
