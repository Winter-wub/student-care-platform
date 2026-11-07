---
status: accepted
date: 2026-11-07
---

# A Menu label save re-links in bulk and retires by exact revision after a confirmed batch

Until #66 a back-office edit of a Menu label reached a Student only at their next `set_menu`, and on LINE each edit left one more rich menu under the same generation, kept on purpose because the adapter could not tell an older version of this Deployment from another Deployment's live one (#63's KEEP rule).

Now each save of a Menu label records the next **Menu revision** of every effective Locale whose labels change (`menu_revision`, migration 0011; revision 0 is the Menu shown until then, with its labels), and enqueues one Kernel Job per such Locale in the same batch as the `setting` write. The Job re-links every linked account of that Locale (`Channel::relink_menu`, chunked and resumable) and only then retires the older revisions (`Channel::retire_menus`), handing the adapter the older revisions as whole Menus. On LINE the revision is part of the rich menu name, so the adapter deletes exactly the names those Menus produce, never a name it was not handed. A save during a run makes a newer revision: the older run stops at its next step and the newer Job re-links everyone and retires every older revision.

On LINE we re-link with `bulk/link` (500 ids a call, 2,000 requests a second) and retire through batch control (`POST /v2/bot/richmenu/batch`, link from each previous rich menu to the new one, then the progress endpoint until `succeeded`, then the deletes). We rejected bulk link alone: it answers 202 and works in the background, and the only way to confirm it is one `GET` per user, so a delete after it could strand accounts on the default rich menu. We rejected batch control alone: it allows 3 requests an hour, so a few edits in a row would delay the new labels by up to an hour; with both, the labels change within a minute and only the cleanup waits for the hourly budget. Batch control also moves accounts the Kernel does not list (a Student whose Locale changed since), which the bulk link cannot reach.

We rejected naming the previous version by revision alone (`harpseal-th-1-r3-*`): two Deployments on one token both reach revision 3, and that would delete the other's. The exact name includes the labels' hash, so only identical labels at the same revision collide, and those already share one rich menu.

## Consequences

- Public API additions: `Menu::with_revision` / `Menu::revision`, `Channel::relink_chunk`, `Channel::relink_menu`, `Channel::retire_menus` (all default-implemented: `set_menu` per account, retire nothing), `MenuRelinked`, `MenuRetire`. LINE: `richmenu::{BULK_LINK_MAX, RELINK_CHUNK, PROGRESS_POLL_SECS, BATCH_RETRY_SECS}`; `richmenu::name_key` also reads revisioned names.
- Every Menu the Kernel sets carries the newest revision, which costs one query per unlocking `set_menu`.
- A revision whose Job fails for good (four failed runs) stays `linking`; the next label save in that Locale re-links everyone and retires it.
- Telegram re-sets the per-chat commands of every linked account (two calls each) and has nothing to retire.
- A save that changes the default Locale's label also re-links the Locales that fall back to it for that key.
