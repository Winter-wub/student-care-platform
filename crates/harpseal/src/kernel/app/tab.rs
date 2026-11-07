//! The per-tab check of a Mini App session page (#86): two tabs share one
//! cookie, so each page of a live session makes sure it is this tab's
//! before it is painted (`ui::mini_app_session` runs [`tab_js`] first).

/// Runs first on every page of a live session (`ui::mini_app_session`),
/// with `@STAMP@` this session's [`tab_stamp`] (#86):
/// - the tab is `pending` (its entry page just exchanged): it takes the
///   stamp;
/// - the tab holds another stamp, or none (the page came from another
///   tab's session, or a tab that never ran the entry page): the page hides
///   before it is painted and reloads with `?launch`, so this tab signs in
///   for itself;
/// - otherwise it ends the entry page's loop guard, and a later restore
///   from the bfcache (no request: the idle lifetime cannot see it)
///   reloads with `?launch`.
///
/// Without `sessionStorage` (blocked) nothing is checked.
const TAB_JS: &str = r#"(function () {
  var p = '@STAMP@', s;
  try { s = sessionStorage.getItem('hs_app_s'); } catch (e) { return; }
  var again = function () {
    document.documentElement.style.visibility = 'hidden';
    location.replace(location.pathname + '?launch');
  };
  if (s === 'pending') { sessionStorage.setItem('hs_app_s', p); }
  else if (s !== p) { again(); return; }
  try { sessionStorage.removeItem('hs_app_x'); } catch (e) {}
  window.addEventListener('pageshow', function (e) { if (e.persisted) { again(); } });
})();"#;

/// The tab stamp of the session whose CSRF token is `csrf`: 16 hex of
/// `SHA-256("app-tab:" + csrf)`, so it says nothing usable about either.
pub(crate) fn tab_stamp(csrf: &str) -> String {
    let mut h = crate::auth::sha256_hex(format!("app-tab:{csrf}").as_bytes());
    h.truncate(16);
    h
}

/// [`TAB_JS`] for the session whose CSRF token is `csrf`.
pub(crate) fn tab_js(csrf: &str) -> String {
    TAB_JS.replace("@STAMP@", &tab_stamp(csrf))
}
