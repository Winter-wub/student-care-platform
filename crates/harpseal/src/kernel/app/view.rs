//! Markup of the Mini App pages: the entry page that starts the exchange,
//! the Student's page, one announcement, and the small answer pages. All
//! in `ui::mini_app_session` (same CSS as the back office).

use axum::http::{HeaderValue, StatusCode, header};
use axum::response::{Html, IntoResponse, Response};
use maud::{Markup, PreEscaped, html};

use super::session::AppVisitor;
use super::{ACK_SUFFIX, external, sdk::Sdks};
use crate::PageState;
use crate::kernel::students::{channel_label, format_ms};
use crate::kernel::text::Texts;
use crate::modules::announcements::StudentAnnouncement;
use crate::rich_text;

/// The body has a link (the page then loads [`external::script`]).
fn has_links(body: &str) -> bool {
    !rich_text::parse(body).links().is_empty()
}

/// Pinned LIFF SDK (LINE's "edge/2" channel).
pub(crate) const LIFF_SDK: &str = "https://static.line-scdn.net/liff/edge/2/sdk.js";
/// Telegram's Web App script.
pub(crate) const TELEGRAM_SDK: &str = "https://telegram.org/js/telegram-web-app.js";

/// Reads the platform token and posts it to `/app/session` as a form. A
/// `sessionStorage` stamp stops a redirect loop when the cookie does not
/// stick (a webview that drops cookies): a second exchange within 15 s
/// shows the "open from the chat" text instead. Outside both chat apps
/// (Telegram reports no platform, LIFF is not in its client and the visit
/// did not come through LIFF) it shows the plain page with the notice and
/// Staff sign-in links; LINE Login is one tap there, never automatic.
const EXCHANGE_JS: &str = r#"(function () {
  var f = document.getElementById('xchg');
  var q = new URLSearchParams(location.search);
  // LIFF redirects to `liff.state` after init: only a plain /app path.
  var st = q.get('liff.state');
  if (st !== null && !/^\/app(\/[A-Za-z0-9_.\-]+)*$/.test(st)) {
    location.replace(location.pathname);
    return;
  }
  var show = function (id) {
    ['xchg-wait', 'xchg-fail', 'xchg-out'].forEach(function (x) {
      document.getElementById(x).hidden = x !== id;
    });
  };
  var fail = function () { show('xchg-fail'); };
  var go = function (kind, token) {
    try {
      var last = +sessionStorage.getItem('hs_app_x') || 0;
      if (Date.now() - last < 15000) { fail(); return; }
      sessionStorage.setItem('hs_app_x', String(Date.now()));
      // The first session page in this tab takes it (tab.rs).
      sessionStorage.setItem('hs_app_s', 'pending');
    } catch (e) {}
    f.elements.channel.value = kind;
    f.elements.token.value = token;
    f.elements.framed.value = window.top !== window.self ? '1' : '';
    f.submit();
  };
  show('xchg-wait');
  var tg = window.Telegram && window.Telegram.WebApp;
  if (f.dataset.tg && tg && tg.initData) { tg.ready(); go(f.dataset.tg, tg.initData); return; }
  var inTg = !!(f.dataset.tg && tg && tg.platform && tg.platform !== 'unknown');
  var lf = window.liff;
  var login = function () { lf.login({ redirectUri: location.href }); };
  var viaLiff = ['liff.state', 'liff.referrer', 'liffClientId', 'liffRedirectUri', 'code']
    .some(function (k) { return q.has(k); }) ||
    /^https:\/\/(liff|miniapp)\.line\.me\//.test(document.referrer);
  if (f.dataset.liff && lf) {
    lf.init({ liffId: f.dataset.liff }).then(function () {
      if (q.has('liff.state')) { return; }
      var t = lf.getIDToken();
      if (t) { go(f.dataset.line, t); }
      else if (lf.isInClient() || inTg) { fail(); }
      else if (viaLiff) { login(); }
      else {
        // Only an initialised SDK can log in.
        var b = document.getElementById('xchg-line');
        if (b) { b.onclick = login; b.hidden = false; }
        show('xchg-out');
      }
    }, function () { if (inTg || viaLiff) { fail(); } else { show('xchg-out'); } });
    return;
  }
  if (inTg) { fail(); } else { show('xchg-out'); }
})();"#;

/// Answer pages clear the loop guard: their retry is a deliberate tap. So
/// does every page of a live session ([`super::tab`]): the stamp only stops a
/// loop that never reaches one, and the next launch (#86) must exchange
/// again at once. An answer page also drops a `pending` tab mark (the
/// exchange it was for ended here): left behind, a later page of another
/// tab's session would adopt it. A real stamp stays.
pub(crate) const CLEAR_GUARD_JS: &str = "try { sessionStorage.removeItem('hs_app_x'); \
     if (sessionStorage.getItem('hs_app_s') === 'pending') { sessionStorage.removeItem('hs_app_s'); } \
     } catch (e) {}";

/// A whole Mini App response: never cached; the `Referer` only to this
/// origin, where it marks a navigation inside the launch for a webview
/// without Fetch Metadata ([`super::launch`]).
pub(crate) fn respond(status: StatusCode, page: Markup) -> Response {
    let mut r = (status, Html(page.into_string())).into_response();
    let h = r.headers_mut();
    h.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    h.insert(
        header::REFERRER_POLICY,
        HeaderValue::from_static("same-origin"),
    );
    r
}

fn page(s: &PageState, t: &Texts, csrf: Option<&str>, title: &str, body: Markup) -> Markup {
    crate::ui::mini_app_session(s, title, csrf, &t.t("kernel.app.notice_link"), body)
}

/// The entry page: no session yet. Loads the platform SDKs of the
/// Deployment's Channels and posts the token with `next`. Without script,
/// or outside both chat apps, it is a plain page with links to the notice
/// and the Staff sign-in.
pub(crate) fn entry(s: &PageState, t: &Texts, sdks: &Sdks, next: &str) -> Response {
    let (line, liff) = sdks.liff.clone().unzip();
    let body = html! {
        // Shown while the SDKs download; without script, the links instead.
        div class="card app-center" id="xchg-wait" { p { (t.t("kernel.app.opening")) } }
        noscript { style { "#xchg-wait{display:none}" } }
        div class="card app-center" id="xchg-fail" hidden { p { (t.t("kernel.app.open_in_chat")) } }
        div class="card app-center" id="xchg-out" hidden {
            (outside(t))
            @if sdks.liff.is_some() {
                button type="button" class="secondary" id="xchg-line" hidden {
                    (t.t("kernel.app.line_login"))
                }
            }
        }
        noscript { div class="card app-center" { (outside(t)) } }
        form id="xchg" method="post" action=(super::SESSION_PATH) hidden
            data-liff=[liff] data-line=[line] data-tg=[sdks.telegram.as_deref()] {
            input type="hidden" name="channel";
            input type="hidden" name="token";
            input type="hidden" name="next" value=(next);
            input type="hidden" name="framed";
        }
        @if sdks.telegram.is_some() { script src=(TELEGRAM_SDK) {} }
        @if sdks.liff.is_some() { script src=(LIFF_SDK) charset="utf-8" {} }
        script { (PreEscaped(EXCHANGE_JS)) }
    };
    respond(
        StatusCode::OK,
        page(s, t, None, &t.t("kernel.app.title"), body),
    )
}

/// What a visitor outside both chat apps sees: where the page lives, the
/// notice and the Staff sign-in.
fn outside(t: &Texts) -> Markup {
    html! {
        p { (t.t("kernel.app.outside")) }
        p { a class="btn" href="/notice" { (t.t("kernel.app.notice_link")) } }
        p { a class="btn secondary" href=(crate::auth::LOGIN_PATH) { (t.t("kernel.app.staff_login")) } }
    }
}

/// A short answer with a button: `retry` reopens that path (the entry
/// page runs again), else the button goes back to `/app`.
pub(crate) fn answer(
    s: &PageState,
    t: &Texts,
    status: StatusCode,
    key: &str,
    retry: Option<&str>,
) -> Response {
    let text = t.t(key);
    let (href, label) = match retry {
        Some(next) => (next, t.t("kernel.app.retry")),
        None => (super::APP_PATH, t.t("kernel.app.back")),
    };
    let body = html! {
        div class="card app-center" {
            p { (text) }
            a class="btn" href=(href) { (label) }
        }
        script { (PreEscaped(CLEAR_GUARD_JS)) }
    };
    respond(status, page(s, t, None, &text, body))
}

/// "Open it again from the chat", no button: the token the webview holds
/// is stale (a Telegram `initData` stays the same while the Mini App is
/// open), so retrying here cannot help.
pub(crate) fn hint(s: &PageState, t: &Texts) -> Response {
    let body = html! {
        div class="card app-center" { p { (t.t("kernel.app.open_in_chat")) } }
        script { (PreEscaped(CLEAR_GUARD_JS)) }
    };
    let title = t.t("kernel.app.title");
    respond(StatusCode::UNAUTHORIZED, page(s, t, None, &title, body))
}

/// The page of an account without a live Link.
pub(crate) fn not_linked(s: &PageState, t: &Texts, csrf: Option<&str>) -> Response {
    let body = html! {
        div class="card app-center" { p { (t.t("kernel.app.not_linked")) } }
        script { (PreEscaped(CLEAR_GUARD_JS)) }
    };
    respond(
        StatusCode::OK,
        page(s, t, csrf, &t.t("kernel.app.title"), body),
    )
}

/// What the Student's page shows besides the header.
pub(crate) struct Home<'a> {
    /// The latest Consent's notice version, if any.
    pub(crate) consent: Option<String>,
    /// The Student accepted an older notice than the current one: the chat
    /// asks again before the next answer (re-consent, #9 step 10).
    pub(crate) reconsent: bool,
    /// (label, path) of every Module's home link, sorted (#72).
    pub(crate) links: &'a [(String, &'static str)],
    /// `None` when the Deployment has no Announcements Module.
    pub(crate) news: Option<&'a [StudentAnnouncement]>,
}

/// `/app` for a linked Student.
pub(crate) fn home(s: &PageState, t: &Texts, v: &AppVisitor, h: &Home<'_>) -> Response {
    let Some(st) = &v.student else {
        return not_linked(s, t, Some(&v.csrf));
    };
    let initial: String = st.name.chars().next().into_iter().collect();
    let channel = channel_label(&v.channel);
    let body = html! {
        section class="card" {
            div class="app-who" {
                div class="avatar" aria-hidden="true" { (initial) }
                div {
                    h1 { (st.name) }
                    p class="muted" { (t.tf("kernel.app.student_code", &[("code", &st.code)])) }
                    p class="muted" { (t.tf("kernel.app.via", &[("channel", &channel)])) }
                }
            }
            p class="muted app-consent" {
                @match &h.consent {
                    Some(version) => {
                        a href="/notice?app=1" {
                            (t.tf("kernel.app.consent", &[("version", version)]))
                        }
                    }
                    None => (t.t("kernel.app.consent_none")),
                }
            }
            @if h.reconsent {
                p class="app-consent app-wait" role="status" { (t.t("kernel.app.consent_pending")) }
            }
            // Ends the App Session (#86), e.g. on a shared computer.
            form class="app-sign-out" method="post" action=(super::SIGN_OUT_PATH) {
                input type="hidden" name="_csrf" value=(v.csrf);
                button type="submit" class="link" { (t.t("kernel.app.sign_out")) }
            }
        }
        @if !h.links.is_empty() {
            nav class="card" aria-label=(t.t("kernel.app.links")) {
                ul class="list app-links" {
                    @for (label, href) in h.links { li { a href=(href) { (label) } } }
                }
            }
        }
        @if let Some(news) = h.news {
            section {
                h2 { (t.t("kernel.app.news")) }
                @if news.is_empty() {
                    p class="card muted" { (t.t("kernel.app.news_empty")) }
                } @else {
                    ul class="app-news" {
                        @for a in news { li id=(format!("a-{}", a.id)) { (item(s, t, v, a)) } }
                    }
                    (external::script(s, &v.channel, news.iter().any(|a| has_links(&a.body))))
                }
            }
        }
    };
    respond(
        StatusCode::OK,
        page(s, t, Some(&v.csrf), &t.t("kernel.app.title"), body),
    )
}

fn date(s: &PageState, ms: i64) -> String {
    format_ms(ms, s.config.utc_offset_minutes().unwrap_or(0))
}

/// One announcement in the list: title, two lines and the date; opens in
/// place (`<details>`, no script) to the whole text and the button.
fn item(s: &PageState, t: &Texts, v: &AppVisitor, a: &StudentAnnouncement) -> Markup {
    html! {
        details {
            summary {
                h3 { (a.title) }
                p class="clamp" { (rich_text::plain(&a.body)) }
                div class="app-meta" {
                    span { (date(s, a.at)) }
                    @if a.needs_ack() { span class="app-wait" { (t.t("kernel.app.waiting")) } }
                    span class="more" { (t.t("kernel.app.read_more")) }
                }
            }
            div class="full" {
                (a.body_html(&t.t("announcements.images.slot")))
                (ack_part(t, v, a))
            }
        }
    }
}

/// The "รับทราบ" form (HTMX swaps it for [`acked`]; without script it
/// posts and comes back to the detail page), or the acknowledged line.
pub(crate) fn ack_part(t: &Texts, v: &AppVisitor, a: &StudentAnnouncement) -> Markup {
    let action = format!("{}/{}{ACK_SUFFIX}", super::NEWS_PATH, a.id);
    html! {
        @if a.needs_ack() {
            form class="app-ack" method="post" action=(action)
                hx-post=(action) hx-target="this" hx-swap="outerHTML" {
                input type="hidden" name="_csrf" value=(v.csrf);
                button type="submit" { (t.t("kernel.app.ack")) }
            }
        } @else if a.require_ack {
            (acked(t))
        }
    }
}

/// The line that replaces the button.
pub(crate) fn acked(t: &Texts) -> Markup {
    html! { p class="app-acked" role="status" { (t.t("kernel.app.acked")) } }
}

/// The line a Student sees instead of an announcement no longer shown:
/// retracted, or expired (#77).
pub(crate) fn gone_key(a: &StudentAnnouncement) -> Option<&'static str> {
    if a.retracted {
        Some("kernel.app.retracted")
    } else if a.expired {
        Some("kernel.app.unavailable")
    } else {
        None
    }
}

/// The page of one announcement (a deep link from a Card).
pub(crate) fn detail(
    s: &PageState,
    t: &Texts,
    v: &AppVisitor,
    a: &StudentAnnouncement,
) -> Response {
    // Retracted or expired (#77): a line instead of the text.
    let gone = gone_key(a);
    let body = if let Some(key) = gone {
        html! { div class="card app-center" { p { (t.t(key)) } } }
    } else {
        html! {
            article class="card" {
                h1 { (a.title) }
                p class="muted" { (date(s, a.at)) }
                (a.body_html(&t.t("announcements.images.slot")))
                (ack_part(t, v, a))
            }
            (external::script(s, &v.channel, has_links(&a.body)))
        }
    };
    let title = if let Some(key) = gone {
        t.t(key)
    } else {
        a.title.clone()
    };
    let body = html! {
        (body)
        p { a class="btn secondary" href=(super::APP_PATH) { (t.t("kernel.app.back")) } }
    };
    respond(StatusCode::OK, page(s, t, Some(&v.csrf), &title, body))
}
