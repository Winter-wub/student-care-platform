//! A body as HTML, for the Mini App and the back-office preview (one
//! function, so the preview is what the Student sees). maud escapes every
//! text; only `https:` URLs that passed [`crate::render::link_url`] reach
//! an `href`.

use maud::{Markup, html};

use super::{Block, Doc, Inline, Item};

impl Doc {
    /// The body as HTML: `<p class="app-body">` for a body without lists
    /// (exactly the markup a plain body always had), else
    /// `<div class="app-body">` holding the text runs and `<ul>`/`<ol>`.
    /// Line breaks are kept as text (`white-space: pre-line`). A link opens
    /// a new window (`target="_blank" rel="noopener noreferrer"`) and
    /// carries `data-ext`, which the Mini App's script turns into the
    /// platform's "open outside" call.
    pub fn html(&self) -> Markup {
        let lists = self.blocks.iter().any(|b| !matches!(b, Block::Text(_)));
        if !lists {
            return html! { p class="app-body" { (blocks(&self.blocks)) } };
        }
        html! { div class="app-body" { (blocks(&self.blocks)) } }
    }
}

/// [`Doc::html`] of `body`.
pub fn html(body: &str) -> Markup {
    super::parse(body).html()
}

fn blocks(blocks: &[Block]) -> Markup {
    html! {
        @for b in blocks {
            @match b {
                Block::Text(lines) => {
                    @for (i, line) in lines.iter().enumerate() {
                        @if i > 0 { "\n" }
                        (pieces(line))
                    }
                }
                Block::Bullets(items) => { ul { (list(items)) } }
                Block::Numbered { start, items } => {
                    @if *start == 1 { ol { (list(items)) } } @else { ol start=(start) { (list(items)) } }
                }
            }
        }
    }
}

fn list(items: &[Item]) -> Markup {
    html! { @for i in items { li { (pieces(&i.content)) } } }
}

fn pieces(ps: &[Inline]) -> Markup {
    html! {
        @for p in ps {
            @match p {
                Inline::Text(t) => { (t) }
                Inline::Bold(c) => { strong { (pieces(c)) } }
                Inline::Italic(c) => { em { (pieces(c)) } }
                Inline::Link { text, url } => {
                    a href=(url) target="_blank" rel="noopener noreferrer" data-ext { (text) }
                }
            }
        }
    }
}
