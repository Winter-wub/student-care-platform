//! Links in an announcement body open outside the Mini App (#108). Each
//! link is `target="_blank" rel="noopener noreferrer"` with `data-ext`
//! ([`crate::rich_text`]); on a page that has one, this script asks the
//! visitor's platform to open it in the outside browser: LIFF's
//! `liff.openWindow({ external: true })`, Telegram's
//! `Telegram.WebApp.openLink`. Without the SDK, or when it fails, the
//! plain link stands (a new window). Only `https:` hrefs are touched; the
//! parser makes no other.

use maud::{Markup, PreEscaped, html};

use super::sdk::Sdks;
use super::view::{LIFF_SDK, TELEGRAM_SDK};
use crate::{PageState, WebClient};

const EXTERNAL_JS: &str = r#"(function () {
  var liffId = document.currentScript.dataset.liff;
  document.addEventListener('click', function (e) {
    var a = e.target.closest && e.target.closest('a[data-ext]');
    if (!a || !/^https:\/\//.test(a.href)) { return; }
    var url = a.href;
    var tg = window.Telegram && window.Telegram.WebApp;
    if (tg && tg.openLink) { e.preventDefault(); tg.openLink(url); return; }
    var lf = window.liff;
    if (lf && liffId) {
      e.preventDefault();
      lf.init({ liffId: liffId }).then(function () {
        lf.openWindow({ url: url, external: true });
      }).catch(function () { window.open(url, '_blank', 'noopener,noreferrer'); });
    }
  });
})();"#;

/// The script for a page whose bodies have links (`links`), with the SDK
/// of the visitor's Channel `channel`; nothing when there is no link.
pub(crate) fn script(s: &PageState, channel: &str, links: bool) -> Markup {
    if !links {
        return html! {};
    }
    let client = Sdks::client_of(&s.deployment, channel);
    let liff = match &client {
        Some(WebClient::Liff { id }) if !id.is_empty() => Some(id.as_str()),
        _ => None,
    };
    html! {
        @if liff.is_some() { script src=(LIFF_SDK) charset="utf-8" defer {} }
        @if matches!(client, Some(WebClient::Telegram)) { script src=(TELEGRAM_SDK) defer {} }
        script data-liff=[liff] { (PreEscaped(EXTERNAL_JS)) }
    }
}

#[cfg(test)]
mod tests {
    use super::EXTERNAL_JS;

    #[test]
    fn only_https_links_are_opened_outside() {
        assert!(EXTERNAL_JS.contains(r"!/^https:\/\//.test(a.href)"));
        assert!(EXTERNAL_JS.contains("external: true"));
        assert!(EXTERNAL_JS.contains("'noopener,noreferrer'"));
    }
}
