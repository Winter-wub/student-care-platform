//! The busy state (#131): a submit that takes a moment shows that it is
//! working and cannot be sent twice. No client library: HTMX's own
//! `htmx-request` class, `aria-busy`, and this small inline script on
//! every back-office page.
//!
//! - A plain form, once it submits, gets `aria-busy="true"`; its submit
//!   buttons get `aria-disabled="true"` and the pressed one `data-busy`
//!   (drawn with a ring in `ui_states.css`). A second submit while it is
//!   busy is cancelled (`preventDefault`). Nothing is `disabled`, so the
//!   pressed button's `name`/`value` (`op=preview` on the branding page)
//!   always reaches the server, and no browser restores a disabled button
//!   on the way back.
//! - Every `pageshow` (a fresh load or one from the back/forward cache)
//!   clears whatever a previous submit left.
//! - An HTMX request sent by a form marks that form the same way until it
//!   ends, and a repeat is cancelled. Its target is never marked: targets
//!   are live regions (`#pick-results`) or polled (the roster progress).
//! - A form with `data-no-busy` is left alone.

/// The script, inline in the back-office document (no build step).
pub(crate) const BUSY_JS: &str = r#"(function(){
var Q='button[type=submit],button:not([type]),input[type=submit]';
function busy(f){return f.getAttribute('aria-busy')==='true';}
function hold(f,by){f.setAttribute('aria-busy','true');
f.querySelectorAll(Q).forEach(function(b){b.setAttribute('aria-disabled','true');});
if(by&&by.form===f)by.setAttribute('data-busy','');}
function release(f){f.removeAttribute('aria-busy');
f.querySelectorAll('[data-busy]').forEach(function(b){b.removeAttribute('data-busy');});
f.querySelectorAll(Q).forEach(function(b){b.removeAttribute('aria-disabled');});}
document.addEventListener('submit',function(e){var f=e.target;
if(f.hasAttribute('data-no-busy')||f.hasAttribute('hx-post')||f.hasAttribute('hx-get'))return;
if(busy(f)){e.preventDefault();return;}
if(!e.defaultPrevented)hold(f,e.submitter);});
addEventListener('pageshow',function(){document.querySelectorAll('form[aria-busy]').forEach(release);});
document.addEventListener('htmx:beforeRequest',function(e){var f=e.detail.elt;
if(!f||f.tagName!=='FORM'||f.hasAttribute('data-no-busy'))return;
if(busy(f)){e.preventDefault();return;}
hold(f,null);});
document.addEventListener('htmx:afterRequest',function(e){var f=e.detail.elt;
if(f&&f.tagName==='FORM')release(f);});
})();"#;

#[cfg(test)]
mod tests {
    use super::BUSY_JS;

    #[test]
    fn the_script_blocks_repeats_without_disabling_anything() {
        // A disabled submitter would drop its name/value from the post.
        assert!(!BUSY_JS.contains("disabled=true") && !BUSY_JS.contains(".disabled"));
        assert!(BUSY_JS.contains("if(busy(f)){e.preventDefault();return;}"));
        // Every pageshow clears, not only one from the back/forward cache.
        assert!(BUSY_JS.contains("addEventListener('pageshow',function(){"));
        assert!(!BUSY_JS.contains("setTimeout"));
        // An HTMX target (a live region, a polled block) is never marked.
        assert!(!BUSY_JS.contains("detail.target"));
        // Nothing that could close the inline `<script>` early.
        assert!(!BUSY_JS.contains("</"));
    }
}
