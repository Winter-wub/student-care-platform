//! A body as HTML, for the Mini App and the back-office preview (one
//! function, so the preview is what the Student sees). maud escapes every
//! text; only `https:` URLs that passed [`crate::render::link_url`] reach
//! an `href`. An image's `src` comes from the caller ([`Doc::html_with`]),
//! never from the body.

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
        self.html_with(&|_| None)
    }

    /// [`Doc::html`], with each `![alt](image:N)` shown as the image `img`
    /// gives for slot `N` (`loading="lazy"`, the alt escaped); a slot it
    /// has nothing for shows nothing.
    pub fn html_with(&self, img: &dyn Fn(u8) -> Option<Img>) -> Markup {
        let lists = self.blocks.iter().any(|b| !matches!(b, Block::Text(_)));
        if !lists {
            return html! { p class="app-body" { (blocks(&self.blocks, img)) } };
        }
        html! { div class="app-body" { (blocks(&self.blocks, img)) } }
    }
}

/// Where an image placed in a body comes from (#109).
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct Img {
    /// The `src`: a path or URL the caller built (never body text).
    pub src: String,
    /// Width and height in pixels, when known (no layout shift).
    pub size: Option<(u32, u32)>,
}

impl Img {
    /// An image at `src`.
    pub fn new(src: impl Into<String>, size: Option<(u32, u32)>) -> Self {
        Self {
            src: src.into(),
            size,
        }
    }

    /// The `<img>` with alt text `alt`.
    pub fn markup(&self, alt: &str) -> Markup {
        let (w, h) = self.size.unzip();
        html! {
            img class="app-img" src=(self.src) alt=(alt) loading="lazy" decoding="async"
                width=[w] height=[h];
        }
    }
}

type Imgs<'a> = &'a dyn Fn(u8) -> Option<Img>;

/// [`Doc::html`] of `body`.
pub fn html(body: &str) -> Markup {
    super::parse(body).html()
}

fn blocks(blocks: &[Block], img: Imgs<'_>) -> Markup {
    html! {
        @for b in blocks {
            @match b {
                Block::Text(lines) => {
                    @for (i, line) in lines.iter().enumerate() {
                        @if i > 0 { "\n" }
                        (pieces(line, img))
                    }
                }
                Block::Bullets(items) => { ul { (list(items, img)) } }
                Block::Numbered(items) => { ol { (list(items, img)) } }
            }
        }
    }
}

fn list(items: &[Item], img: Imgs<'_>) -> Markup {
    // A numbered item shows the number typed, as the chat text does.
    html! { @for i in items { li value=[i.number] { (pieces(&i.content, img)) } } }
}

fn pieces(ps: &[Inline], img: Imgs<'_>) -> Markup {
    html! {
        @for p in ps {
            @match p {
                Inline::Text(t) => { (t) }
                Inline::Bold(c) => { strong { (pieces(c, img)) } }
                Inline::Italic(c) => { em { (pieces(c, img)) } }
                Inline::Link { text, url } => {
                    a href=(url) target="_blank" rel="noopener noreferrer" data-ext { (text) }
                }
                Inline::Image { alt, n } => {
                    @if let Some(i) = img(*n) { (i.markup(alt)) }
                }
            }
        }
    }
}
