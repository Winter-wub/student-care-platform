//! A rich menu that exists without its image (#98): uploading the image
//! of a new rich menu, and repairing a reused one that has none (the rules
//! are in [`crate::richmenu`]).

use harpseal::Result;
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

    /// After LINE refused to link the reused rich menu `id` (named `name`)
    /// with a 400: uploads `image` if it has none, and says so (`true`:
    /// link again). `false`: it has its image, so the 400 is about
    /// something else. If the upload fails, the rich menu is deleted (it
    /// cannot be linked, nobody is on it) and forgotten, so that the next
    /// call creates it cleanly.
    pub(super) async fn restore_image(&self, name: &str, id: &str, image: &[u8]) -> Result<bool> {
        if self.has_image(id).await? {
            return Ok(false);
        }
        if let Err(e) = self.upload_image(id, image).await {
            self.menus.forget(name);
            self.delete_rich_menu(id).await;
            return Err(e);
        }
        Ok(true)
    }
}
