//! A rich menu that exists without its image (#98): uploading the image
//! of a new rich menu, and repairing a reused one that has none (the rules
//! are in [`crate::richmenu`]).

use harpseal::{Error, Result};
use http::Method;

use super::fail;
use crate::LineChannel;
use crate::api::{API_DATA, path_id};

impl LineChannel {
    /// `POST /v2/bot/richmenu/{id}/content` with `image`
    /// (<https://developers.line.biz/en/reference/messaging-api/#upload-rich-menu-image>).
    pub(super) async fn upload_image(&self, id: &str, image: &[u8]) -> Result<()> {
        let url = format!("{API_DATA}/v2/bot/richmenu/{}/content", path_id(id)?);
        self.call(Method::POST, &url, Some("image/png"), image.to_vec())
            .await?
            .map(drop)
            .map_err(fail)
    }

    /// Whether rich menu `id` has an image: `GET
    /// /v2/bot/richmenu/{id}/content` answers 404 for a rich menu without
    /// one (or none at all)
    /// (<https://developers.line.biz/en/reference/messaging-api/#download-rich-menu-image>).
    async fn has_image(&self, id: &str) -> Result<bool> {
        let url = format!("{API_DATA}/v2/bot/richmenu/{}/content", path_id(id)?);
        match self.call(Method::GET, &url, None, Vec::new()).await? {
            Ok(_) => Ok(true),
            Err(f) if f.status == 404 => Ok(false),
            Err(f) => Err(fail(f)),
        }
    }

    /// After LINE refused to link the reused rich menu `id` with a 400
    /// (`refusal`, LINE's message): uploads `image` if it has none, and says
    /// so (`true`: link again). `false`: it has its image, so the 400 is
    /// about something else. LINE never replaces an image ("An image has
    /// already been uploaded to the richmenu", 400), so when another isolate
    /// or Deployment repairs the same rich menu first, this upload fails: the
    /// image is checked again and, if it is there now, that is a repair too.
    /// Otherwise the upload's error is returned. The rich menu is never
    /// deleted here: others may be linked to it (the KEEP rule).
    ///
    /// The check downloads the image (up to 1 MB), the only way LINE offers
    /// to ask; it runs only after such a 400, once per call. If the check
    /// itself fails, the error keeps the 400.
    pub(super) async fn restore_image(
        &self,
        id: &str,
        image: &[u8],
        refusal: &str,
    ) -> Result<bool> {
        match self.has_image(id).await {
            Ok(true) => return Ok(false),
            Ok(false) => {}
            Err(e) => {
                return Err(Error::Platform(format!(
                    "LINE answered 400: {refusal}; checking the image of {id} failed: {e}"
                )));
            }
        }
        match self.upload_image(id, image).await {
            Ok(()) => Ok(true),
            // Lost the race: someone else's image is there now.
            Err(_) if matches!(self.has_image(id).await, Ok(true)) => Ok(true),
            Err(e) => Err(e),
        }
    }
}
