//! [`CardImage`]: the image at the top of a [`super::Card`] (#109).

/// The image at the top of a [`super::Card`] (#109): a JPEG or PNG the platform
/// fetches itself from an https URL, without a session. Build it with
/// [`CardImage::new`], which refuses a URL that fails
/// [`crate::render::link_url`]; every adapter checks again
/// ([`CardImage::safe_url`]) and leaves the image out when it fails.
/// Degrade rules (ADR-0003): LINE puts it in the Flex bubble's hero
/// (20:13, cover), Telegram as the large link preview above the text, a
/// Channel with neither leaves it out; the text never depends on it.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct CardImage {
    /// The `https:` URL of the image.
    pub url: String,
    /// What the image shows, for platforms with alt text (may be empty).
    pub alt: String,
}

impl CardImage {
    /// An image at `url`; refuses a URL that is not `https:`, has user
    /// info, unsafe characters or more than [`crate::render::URL_MAX`]
    /// bytes ([`crate::render::link_url`]).
    pub fn new(url: impl Into<String>, alt: impl Into<String>) -> crate::Result<Self> {
        let url = url.into();
        if !crate::render::link_url(&url) {
            return Err(crate::Error::Invalid(
                "a Card image needs an https: URL".into(),
            ));
        }
        Ok(Self {
            url,
            alt: crate::render::clean_label(&alt.into()),
        })
    }
    /// The URL when it passes [`crate::render::link_url`] (what an
    /// adapter renders); `None` for a URL it must leave out.
    pub fn safe_url(&self) -> Option<&str> {
        crate::render::link_url(&self.url).then_some(self.url.as_str())
    }
}
