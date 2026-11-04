# Research: platform and stack facts the first slice depends on

Answers issue #2 (`Winter-wub/student-care-platform#2`). Compiled 2026-11-05 from primary sources only (vendor docs, vendor-owned source repositories, the crates.io and npm registries). Each fact is one bullet followed by its source URL. A fact marked **UNCONFIRMED** could not be verified from a primary source; the bullet says what was tried.

Vocabulary follows `GLOSSARY.md` (Channel, Channel Account, Push, Reply, Quota, Choices, Action, Mini App, App Session, Job, Verification Method) and the ADRs in `docs/adr/` (ADR-0002 Cloudflare-first but self-hostable; ADR-0004 server-rendered back office; ADR-0005 open source core on a public registry). Note: ADR-0002/0004/0005 are written for a TypeScript stack, and the owner has since switched to Rust, see section 4..

How to read the markers: **CONFIRMED** = stated in the cited page or source file. **INFERENCE** = our reading of confirmed facts, not stated by the source. **UNCONFIRMED** = could not be verified.

---

## 1. LINE (ไลน์)

### 1.1 File size ceiling for what a Student sends

- **UNCONFIRMED.** No page in the Messaging API docs gives a maximum size for a file, image, video or audio that a user sends to an Official Account. Searched: the full Messaging API reference (Markdown edition), `receiving-messages`, and LINE Help Center search results. The "Max file size" lines in the reference (1 MB, 10 MB, 200 MB) apply to messages the bot sends, not to what users send. https://developers.line.biz/en/reference/messaging-api/index.html.md
- The `file` webhook message object carries `fileName` and `fileSize` (bytes), so the bot can reject or skip an oversize file before downloading it. https://developers.line.biz/en/reference/messaging-api/#wh-file
- Content is fetched from `https://api-data.line.me/v2/bot/message/{messageId}/content` (a different host from `api.line.me`). It works only when `contentProvider.type` is `line`. https://developers.line.biz/en/reference/messaging-api/#get-content
- Large video/audio can answer `202` while LINE prepares the binary; poll "Verify the preparation status" before downloading. https://developers.line.biz/en/reference/messaging-api/#get-content
- "Content sent by users may be transformed internally, such as shrinking." The docs promise no original-quality bytes. https://developers.line.biz/en/reference/messaging-api/#get-content
- Content is "automatically deleted after a certain period" with "no guarantee for how long content is stored", and the period is not published. https://developers.line.biz/en/reference/messaging-api/#get-content
- Text cannot be re-fetched: "There is no API to get the text sent by users again after receiving the webhook." https://developers.line.biz/en/reference/messaging-api/#get-content
- Request bodies sent to the Messaging API are capped at 2 MB (`413 Payload Too Large`). https://developers.line.biz/en/reference/messaging-api/#status-codes

### 1.2 Free plan of 300 messages/month: does sending stop at the limit?

- The Thai Free plan allows 300 messages per month (Basic 15,000 at 1,280 THB; Pro 35,000 at 1,780 THB, excl. 7% VAT). Free: "เมื่อคุณเปิดบัญชีทางการจะได้รับข้อความเพื่อส่งฟรี 300 ข้อความต่อเดือน". https://lineforbusiness.com/th/service/line-oa-features/broadcast-message
- The Free allowance was cut from 500 to 300 per month, effective 1 August 2024. https://lineforbusiness.com/th/news/20240619_1
- The Thai plan page lists no extra-message price for Free, so Free has no paid overage. Basic is 0.1 THB and Pro 0.06 THB per extra message. https://lineforbusiness.com/th/service/line-oa-features/broadcast-message
- **Sending stops.** "When you exceed the limit of messages that can be sent in a month, an error response will be returned and the messages won't be sent." https://developers.line.biz/en/docs/messaging-api/pricing/#when-you-exceed-the-limit-of-free-messages
- The error is `429 Too Many Requests`, whose listed causes include "Exceeded the number of free messages" and "Exceeded your maximum number of additional messages allowed to be sent". https://developers.line.biz/en/reference/messaging-api/#status-codes
- The same `429` "may" also appear while messages remain this month (the reference points to an FAQ entry), so the adapter must not treat every `429` as "Quota exhausted". https://developers.line.biz/en/reference/messaging-api/#status-codes
- Counted against the plan: Push, Multicast, Broadcast, Narrowcast. Not counted: Reply messages. https://developers.line.biz/en/docs/messaging-api/pricing/#sending-methods-that-are-counted-towards-the-number-of-messages-for-the-subscription-plan
- Counting is per recipient: one push of 4 message objects to a chat of 5 people is 5 messages. Messages to users who blocked the account or to non-existent user IDs are not counted. https://developers.line.biz/en/docs/messaging-api/pricing/#how-to-count-the-number-of-messages-sent
- Usage can be read from the API: "Get the target limit for sending messages this month" (`#get-quota`) and "Get number of messages sent this month" (`#get-consumption`). https://developers.line.biz/en/docs/messaging-api/pricing/#when-you-exceed-the-limit-of-free-messages
- **UNCONFIRMED:** whether a Reply still works after the free Push limit is exhausted. The pricing page says Replies are not counted but does not say they keep working past the limit. Test on a real Official Account before relying on it.
- **UNCONFIRMED:** the day/time zone on which the monthly count resets. Not stated on the pages above.

### 1.3 Rich menu: link/unlink per user and in bulk

- Link one user: `POST https://api.line.me/v2/bot/user/{userId}/richmenu/{richMenuId}`. Only one rich menu per user; linking again replaces it. Takes effect immediately. https://developers.line.biz/en/reference/messaging-api/#link-rich-menu-to-user
- Unlink one user: `DELETE https://api.line.me/v2/bot/user/{userId}/richmenu`. https://developers.line.biz/en/reference/messaging-api/#unlink-rich-menu-from-user
- Priority: per-user menu (Messaging API) over default menu (Messaging API) over default menu set in LINE Official Account Manager. https://developers.line.biz/en/reference/messaging-api/#link-rich-menu-to-user
- Only friends can be linked. For users who deleted their account, blocked the account, never added it, or whose ID belongs to another provider, the single-user call still returns `200` but nothing is linked. Treat a `200` as "maybe" and verify with "Get rich menu ID of user". https://developers.line.biz/en/reference/messaging-api/#link-rich-menu-to-user
- Bulk link: `POST https://api.line.me/v2/bot/richmenu/bulk/link` with `richMenuId` and `userIds` (max 500 user IDs per request). Processed asynchronously; `202` does not guarantee linking, "normally ... within a few seconds". https://developers.line.biz/en/reference/messaging-api/#link-rich-menu-to-users
- Bulk unlink: `POST .../richmenu/bulk/unlink`, also asynchronous with `202`. https://developers.line.biz/en/reference/messaging-api/#unlink-rich-menu-from-users
- Rate limit for per-user link, bulk link and bulk unlink: 2,000 requests per second. https://developers.line.biz/en/reference/messaging-api/#link-rich-menu-to-user
- Replace or unlink every user of one menu in one call: `POST /v2/bot/richmenu/batch`, rate limit 3 requests per hour. https://developers.line.biz/en/reference/messaging-api/#rate-limits
- Creating/deleting rich menus via the API: 100 requests per hour. Rich menus made in LINE Official Account Manager are exempt. https://developers.line.biz/en/reference/messaging-api/#rate-limits
- A default (all-users) menu set via the Messaging API can take up to a minute to appear; per-user is immediate. A deleted menu may stay visible in an already-open chat. https://developers.line.biz/en/docs/messaging-api/rich-menus-overview/
- Users who are not friends see the default menu; you cannot link a menu to them. https://developers.line.biz/en/docs/messaging-api/rich-menus-overview/

### 1.4 Webhook signature verification

- LINE sends header `x-line-signature` = HMAC-SHA256 of the request body with the **channel secret** as key, base64-encoded. https://developers.line.biz/en/docs/messaging-api/verify-webhook-signature/#line-platform-sends-webhook-request
- Verify against the exact raw body string: any deserialising, escaping or string substitution before verifying makes valid requests fail. https://developers.line.biz/en/docs/messaging-api/verify-webhook-signature/#receiving-webhook-request
- Missing or non-matching signature: do not process, end with an error. https://developers.line.biz/en/docs/messaging-api/verify-webhook-signature/#signature-validation
- LINE does not publish webhook source IPs; verify the signature instead of filtering by IP. https://developers.line.biz/en/docs/messaging-api/verify-webhook-signature/
- Reissuing the channel secret immediately invalidates the old one and needs channel Admin rights. https://developers.line.biz/en/docs/messaging-api/verify-webhook-signature/#reissue-channel-secret
- Reference sample (Java/Ruby/etc.) computes `base64(HMAC_SHA256(channelSecret, body))` and compares. https://developers.line.biz/en/reference/messaging-api/#signature-validation
- **INFERENCE for the adapter:** read the body with `await c.req.text()` before any JSON parsing and compute with WebCrypto `HMAC`/`SHA-256`, which runs on both Workers and Node. (Same technique as the tested Telegram snippet in 2.2.)

### 1.5 LIFF: same `userId` as the Messaging API channel

- User IDs differ per provider, but within one provider they are identical across channel types: "if there is a Messaging API channel and a LINE Login channel under the same provider, the user ID ... is the same value." https://developers.line.biz/en/docs/messaging-api/getting-user-ids/
- A LIFF app belongs to a **LINE Login** channel, not a Messaging API channel. Add it in the console's LIFF tab; up to 30 LIFF apps per channel. https://developers.line.biz/en/docs/liff/registering-liff-apps/
- Therefore: create the LINE Login channel and the Messaging API channel under the **same provider**. A user ID from a channel under another provider is a different value, and such IDs cause silent no-ops in rich menu calls (1.3). https://developers.line.biz/en/docs/messaging-api/getting-user-ids/
- User IDs match `U[0-9a-f]{32}` and "can't be changed"; display name and picture can change and must not identify a user. https://developers.line.biz/en/docs/messaging-api/getting-user-ids/ and https://developers.line.biz/en/docs/line-login/managing-users/
- LINE recommends creating new LIFF apps as LINE MINI Apps because LIFF is to be integrated into the LINE MINI App brand. Check this before fixing the Mini App choice. https://developers.line.biz/en/docs/liff/registering-liff-apps/
- Endpoint URL must be `https`, with no URL fragment. Opening goes through `https://liff.line.me/{liffId}`. https://developers.line.biz/en/docs/liff/registering-liff-apps/

### 1.6 ID token request and verify (App Session)

- Select the `openid` scope when registering the LIFF app; without it, or without user consent, no ID token is available. https://developers.line.biz/en/docs/liff/developing-liff-apps/#getting-tokens
- In the page: `liff.getIDToken()` returns the raw ID token (a JWT). Inside the LIFF browser the SDK obtains it at `liff.init()`; in an external browser only after `liff.login()`. https://developers.line.biz/en/reference/liff/#get-id-token
- The ID token is valid for one hour after issue. https://developers.line.biz/en/reference/liff/#get-id-token
- Send the **raw ID token** to the server and have the server verify it. Do not send the decoded profile from `liff.getDecodedIDToken()` or `liff.getProfile()`. https://developers.line.biz/en/docs/liff/using-user-profile/
- Verify endpoint: `POST https://api.line.me/oauth2/v2.1/verify`, form fields `id_token`, `client_id` (the LINE Login channel ID), optional `nonce` and `user_id`. Success returns the payload (`iss`, `sub`, `aud`, `exp`, ...). `sub` is the user ID. https://developers.line.biz/en/reference/line-login/#verify-id-token
- Offline alternative: LIFF ID tokens use `ES256`; verify with the keys at `https://api.line.me/oauth2/v2.1/certs` selected by header `kid`. Web login uses `HS256` with the channel secret. Expected `iss` is `https://access.line.me`. https://developers.line.biz/en/docs/line-login/verify-id-token/#signature
- **INFERENCE:** an App Session resolves as: verify token → `sub` = Channel Account id → Link → Student.

### Implications for the spec (LINE)

Quota must be a first-class concept: a Free account can Push only 300 recipients in a month, so a single announcement to 300 Students uses the whole allowance, and the adapter must surface the `429` as a Quota event rather than retry it. Use Multicast (max 500 recipients) for announcements, and keep Replies as the preferred path since they are not counted. Rich menu changes for a Student should be single-user calls (immediate, 2,000/s) with a read-back; bulk calls are asynchronous and need verification. The spec must require one provider for the Messaging API and LINE Login channels, and the file-size ceiling has to be set by us (check `fileSize` before downloading), because LINE does not document one. Record "LIFF vs LINE MINI App" as a decision to revisit.

---

## 2. Telegram (เทเลแกรม)

### 2.1 Inline keyboard size

- `InlineKeyboardMarkup` is `inline_keyboard`: an array of button rows (arrays of `InlineKeyboardButton`). https://core.telegram.org/bots/api#inlinekeyboardmarkup
- `callback_data` is 1-64 bytes. This is the hard cap on an Action reference (GLOSSARY: Action is referenced by a short id). https://core.telegram.org/bots/api#inlinekeyboardbutton
- **UNCONFIRMED: the maximum number of buttons.** The Bot API reference states no limit on buttons per row or per keyboard. Third-party sources repeat "100 total, 8 per row", but no Telegram-owned page says so, and the TDLib `ReplyMarkup.cpp` source contains no such constant. Treat 100 total / 8 per row as a conservative design ceiling and test against a real bot. https://core.telegram.org/bots/api#inlinekeyboardmarkup
- After a button press, clients show a progress indicator until the bot calls `answerCallbackQuery`, so always answer, even with no text. https://core.telegram.org/bots/api#callbackquery
- `answerCallbackQuery` `text` is 0-200 characters; `show_alert` shows a modal. https://core.telegram.org/bots/api#answercallbackquery

### 2.2 Web App (Mini App) `initData` validation

- Send the whole `Telegram.WebApp.initData` string (a URL query string) to the backend. Do not trust `initDataUnsafe`. https://core.telegram.org/bots/webapps#validating-data-received-via-the-mini-app
- Algorithm (bot-token check): `data_check_string` = all received fields except `hash`, as `key=<value>`, sorted alphabetically, joined by `\n`. `secret_key = HMAC_SHA256(key="WebAppData", message=<bot_token>)`. Valid when `hex(HMAC_SHA256(key=secret_key, message=data_check_string)) == hash`. https://core.telegram.org/bots/webapps#validating-data-received-via-the-mini-app
- Also check `auth_date` (Unix time) to reject stale data. The docs give no maximum age; the freshness window is our choice. https://core.telegram.org/bots/webapps#validating-data-received-via-the-mini-app
- A third party that does not hold the bot token can verify the `signature` field (Ed25519, base64url) with Telegram's published public key, prepending `<bot_id>:WebAppData`. https://core.telegram.org/bots/webapps#validating-data-for-third-party-use
- Mini Apps opened from an **inline button** or the **menu button** receive user info and `query_id`. A Mini App opened from a **keyboard button** instead returns data to the bot with `sendData`. Use inline or menu buttons for App Sessions. https://core.telegram.org/bots/webapps#implementing-mini-apps
- Code (WebCrypto only, runs on Workers and Node with `globalThis.crypto`). Run against an independent `node:crypto` implementation of the documented algorithm on Node 24.14: valid, tampered-hash and stale-`auth_date` inputs gave the expected results. It was **not** run against a Telegram-issued sample, so test once with a real `initData`.

```ts
const enc = new TextEncoder();
const hex = (b: ArrayBuffer) =>
  [...new Uint8Array(b)].map((x) => x.toString(16).padStart(2, "0")).join("");

async function hmac(key: ArrayBuffer | Uint8Array, data: Uint8Array) {
  const k = await crypto.subtle.importKey(
    "raw", key, { name: "HMAC", hash: "SHA-256" }, false, ["sign"]);
  return crypto.subtle.sign("HMAC", k, data);
}

export async function validateInitData(initData: string, botToken: string, maxAgeSeconds = 3600) {
  const params = new URLSearchParams(initData);
  const hash = params.get("hash");
  if (!hash) return null;
  params.delete("hash");
  const dataCheckString = [...params.entries()]
    .map(([k, v]) => `${k}=${v}`).sort().join("\n");
  const secretKey = await hmac(enc.encode("WebAppData"), enc.encode(botToken));
  const calc = hex(await hmac(new Uint8Array(secretKey), enc.encode(dataCheckString)));
  if (calc.length !== hash.length) return null;
  let diff = 0; // constant-time compare
  for (let i = 0; i < calc.length; i++) diff |= calc.charCodeAt(i) ^ hash.charCodeAt(i);
  if (diff !== 0) return null;
  const authDate = Number(params.get("auth_date"));
  if (!authDate || Date.now() / 1000 - authDate > maxAgeSeconds) return null;
  return { user: JSON.parse(params.get("user") ?? "null"), authDate, queryId: params.get("query_id") };
}
```

### 2.3 `setChatMenuButton` and `setMyCommands` per chat

- `setChatMenuButton` takes an optional `chat_id` of a **private chat**; omit it to change the bot's default menu button. `menu_button` defaults to `MenuButtonDefault`. https://core.telegram.org/bots/api#setchatmenubutton
- Menu button can launch a Mini App (text + URL). Per-user buttons need the API method; BotFather's `/setmenubutton` only sets it for everyone. https://core.telegram.org/bots/webapps#launching-mini-apps-from-the-menu-button
- `setMyCommands`: up to 100 commands, with optional `scope` and `language_code`. https://core.telegram.org/bots/api#setmycommands
- Scopes: Default, AllPrivateChats, AllGroupChats, AllChatAdministrators, **Chat** (one chat), ChatAdministrators, ChatMember (7 total). https://core.telegram.org/bots/api#botcommandscope
- Resolution order for what a user sees runs from the most specific scope (Chat + `language_code`) down to Default; the first list that was set wins. https://core.telegram.org/bots/api#determining-list-of-commands
- **INFERENCE:** the Menu (GLOSSARY: "set per Channel Account; locked until the Student has a Link") maps to `BotCommandScopeChat` + `setChatMenuButton(chat_id)` per Telegram private chat. A locked state is a smaller command list or a button that opens the Link flow.

### 2.4 Webhook `secret_token`

- `setWebhook` accepts `secret_token` (1-256 characters, only `A-Z a-z 0-9 _ -`). Each webhook request then carries header `X-Telegram-Bot-Api-Secret-Token` with that value. https://core.telegram.org/bots/api#setwebhook
- Compare the header to the stored secret and reject if absent or different. https://core.telegram.org/bots/api#setwebhook
- A non-2xx response makes Telegram retry and "give up after a reasonable amount of attempts" (count unpublished). https://core.telegram.org/bots/api#setwebhook
- Webhook ports: 443, 80, 88, 8443. `max_connections` 1-100, default 40. https://core.telegram.org/bots/api#setwebhook
- While a webhook is set, `getUpdates` does not work ("You will not be able to receive updates using getUpdates for as long as an outgoing webhook is set up"). **INFERENCE:** local development can use `deleteWebhook` plus long polling. https://core.telegram.org/bots/api#setwebhook

### 2.5 `deleteMessage` / `editMessageText` limits that affect an acknowledge ("รับทราบ") button

- `deleteMessage`: a message can only be deleted if sent less than **48 hours** ago; bots can delete their own outgoing messages and incoming messages in **private chats**. https://core.telegram.org/bots/api#deletemessage
- `editMessageText`: takes `reply_markup` (inline keyboard only) and `text` 1-4096 characters after entity parsing. The documented time limit applies only to business messages not sent by the bot (48 hours); no limit is stated for the bot's own messages. https://core.telegram.org/bots/api#editmessagetext
- **UNCONFIRMED:** that editing a bot's own message older than 48 hours works. The docs state no limit, but we did not test it.
- **UNCONFIRMED:** the error returned when an edit leaves content unchanged. The Bot API reference does not document it. Make the adapter tolerate it.
- **INFERENCE for the acknowledge flow:** on button press call `answerCallbackQuery`, then edit the message (replace the keyboard with a "รับทราบแล้ว" line) instead of deleting it. Edit works at any age as far as documented; delete fails after 48 hours.
- Bots can currently send files of up to 50 MB (the FAQ's wording is about sending/uploading; the download limit for files a Student sends is not stated on that page, so it is **UNCONFIRMED**). https://core.telegram.org/bots/faq#how-do-i-upload-a-large-file

### 2.6 Broadcast limits (for the 300-Student send)

- Single chat: avoid more than one message per second; groups: 20 messages per minute; bulk: about 30 messages per second, otherwise `429`. https://core.telegram.org/bots/faq#my-bot-is-hitting-limits-how-do-i-avoid-this
- Paid broadcasts (up to 1,000/s) require at least 100,000 Stars and 100,000 monthly active users, so they are out of reach for this customer. https://core.telegram.org/bots/faq#how-can-i-message-all-of-my-bots-subscribers-at-once
- Telegram has no per-month message Quota like LINE's. Sending is free by default. https://core.telegram.org/bots/faq#my-bot-is-hitting-limits-how-do-i-avoid-this

### Implications for the spec (Telegram)

Choices must render within a 64-byte `callback_data`, so Actions are stored server-side and referenced by a short id (already in GLOSSARY). The button-count cap is unknown, so ADR-0003's adapter rule should set a conservative cap (suggest 8 per row, 100 total) and test it. Mini App sessions use inline/menu buttons and the HMAC check above, with a freshness window we choose. Webhook handlers must check `X-Telegram-Bot-Api-Secret-Token`. The acknowledge flow should edit, not delete. A 300-Student Push is about 10 seconds of sending at 30/s, which fits the Job model but not a single naive loop on a free Workers plan (see 3.6).

---

## 3. Workers vs Node (เวิร์กเกอร์ส กับ โนด)

### 3.1 Password hashing on Workers

- Argon2 is not available: Workers' `node:crypto` supports everything except a short list that includes "`argon2` and `argon2Sync` are not supported". https://developers.cloudflare.com/workers/runtime-apis/nodejs/crypto/
- `node:crypto` (with `nodejs_compat`) otherwise "fully supported", which includes `scrypt`. https://developers.cloudflare.com/workers/runtime-apis/nodejs/crypto/
- WebCrypto supports PBKDF2 (`deriveBits` / `deriveKey`), with no compatibility flag. https://developers.cloudflare.com/workers/runtime-apis/web-crypto/#supported-algorithms
- Workers enforces a hard upper bound on PBKDF2 iterations, a deliberate deviation from the WebCrypto spec: requests above it fail with `NotSupportedError: Pbkdf2 failed: iteration counts above <max> are not supported`. There is a similar cost check for scrypt (`checkScryptLimits`). https://github.com/cloudflare/workerd/blob/main/src/workerd/api/crypto/impl.c%2B%2B and https://github.com/cloudflare/workerd/blob/main/src/workerd/api/crypto/pbkdf2.c%2B%2B
- **UNCONFIRMED: the numeric cap.** Neither workerd's open source nor Cloudflare's docs state the value (it comes from a limit enforcer outside the open source). Secondary sources (blogs, not Cloudflare) report 100,000 for PBKDF2. Check with a deployed Worker (`wrangler dev` may differ from production) before choosing parameters.
- CPU time: Workers Free is 10 ms per request; Workers Paid defaults to 30 s (configurable to 5 min). Waiting on I/O does not count. The page does not say whether `crypto.subtle` work counts. Assume it does. https://developers.cloudflare.com/workers/platform/limits/#cpu-time
- **INFERENCE:** a strong password hash on the Free plan is unlikely to fit in 10 ms. Staff sign-in hashing implies Workers Paid or a hash that is cheap enough for the Free plan. Hash parameters and algorithm should be stored with each hash so Node deployments can use `scrypt` or Argon2 and Workers deployments PBKDF2/scrypt, and both can verify each other's records.

### 3.2 Sending email from Workers (for the email OTP Verification Method)

- Workers cannot open outbound TCP on port 25: "By default, Workers cannot create outbound TCP connections on port `25` to send email to SMTP mail servers." https://developers.cloudflare.com/workers/runtime-apis/tcp-sockets/
- Cloudflare Email Service (Email Sending, **Beta**) sends from a Worker via the `send_email` binding, a REST API, or authenticated SMTP at `smtp.mx.cloudflare.net:465` (implicit TLS only). https://developers.cloudflare.com/email-service/ and https://developers.cloudflare.com/email-service/api/send-emails/smtp/
- Sending to arbitrary recipients requires Workers Paid: 3,000 emails/month included, then $0.35 per 1,000. Sending to verified destination addresses is free on all plans. https://developers.cloudflare.com/email-service/platform/pricing/
- Before a sending domain is onboarded, mail goes only to verified destination addresses. Limits: 50 recipients per email, 25 MiB per message. Daily quota starts low and grows with reputation. https://developers.cloudflare.com/email-service/platform/limits/
- HTTP email APIs work from Workers as plain `fetch`. Cloudflare documents tutorials for Resend and Postmark. https://developers.cloudflare.com/workers/tutorials/send-emails-with-resend/ and https://developers.cloudflare.com/workers/tutorials/send-emails-with-postmark/
- Workers do have a TCP socket API, but sockets must be created inside a handler (not global scope) and connections to Cloudflare IP ranges are blocked, in addition to port 25. A direct SMTP relay at a university is therefore not a dependable path from Workers. https://developers.cloudflare.com/workers/runtime-apis/tcp-sockets/
- **INFERENCE:** self-hosted Deployments (Node) can talk SMTP to the university's relay with any SMTP client library. This was not checked against a library's documentation.
- **INFERENCE (design):** one `EmailSender` interface (ADR-0002 "behind interfaces") with two adapters: Workers uses the Cloudflare `send_email` binding or an HTTP API (Resend, Postmark); Node uses SMTP to the customer's relay. Email OTP then works on both without SMTP from Workers.
- **UNCONFIRMED:** whether the Cloudflare Email Service Beta terms and Thai-domain deliverability suit this customer. Not covered by the docs.

### 3.3 D1 export and backup API

- Time Travel: always on, point-in-time restore for any minute in the last 30 days (Paid) or 7 days (Free), no extra cost. Max 10 restores per 10 minutes per database. https://developers.cloudflare.com/d1/reference/time-travel/ and https://developers.cloudflare.com/d1/platform/limits/
- CLI export: `wrangler d1 export <db> --remote --output=./database.sql` (also `--table`, `--no-data`, `--no-schema`). https://developers.cloudflare.com/d1/best-practices/import-export-data/#export-an-existing-d1-database
- REST export: `POST /accounts/{account_id}/d1/database/{database_id}/export` with `output_format: "polling"`, polled with `current_bookmark`; it returns a URL to download the SQL. https://developers.cloudflare.com/api/resources/d1/subresources/database/methods/export/
- During export "your D1 will be unavailable to serve queries"; a running export blocks other requests. https://developers.cloudflare.com/api/resources/d1/subresources/database/methods/export/ and https://developers.cloudflare.com/d1/best-practices/import-export-data/
- Export is not supported for databases with virtual tables (such as FTS5). https://developers.cloudflare.com/d1/best-practices/import-export-data/
- Import via `wrangler d1 execute --file` is limited to 5 GiB. A raw `.sqlite3` file cannot be imported directly; convert it to SQL first. https://developers.cloudflare.com/d1/best-practices/import-export-data/
- Database size cap 10 GB (Paid) / 500 MB (Free). Query result cap 50 MB. A row or BLOB is at most 2 MB. https://developers.cloudflare.com/d1/platform/limits/
- **INFERENCE (Kernel "backup" and "import/export"):** a nightly Job calls the REST export and writes the SQL to R2 (via the S3 adapter). The same logical SQL dump format is available from SQLite on Node (`.dump`), so a backup from one runtime can restore to the other. Avoid FTS5 virtual tables in the schema (ADR-0002 already limits us to SQLite features) or document the workaround.

### 3.4 Cron Triggers

- Cron Triggers run on UTC. https://developers.cloudflare.com/workers/configuration/cron-triggers/
- Count per account: 5 (Free) / 250 (Paid). https://developers.cloudflare.com/workers/platform/limits/#account-plan-limits
- Limits per Cron Trigger run: 10 ms CPU on Free. On Paid, 30 s CPU for intervals under 1 hour and 15 min CPU for intervals of 1 hour or more. Wall-clock is 15 min. https://developers.cloudflare.com/workers/platform/limits/#cpu-time
- **UNCONFIRMED:** exactly-once delivery. The Cron docs reviewed do not say whether a trigger can fire twice. Jobs must be idempotent anyway. `controller.noRetry()` exists. https://developers.cloudflare.com/workers/configuration/cron-triggers/
- **INFERENCE:** a single Cron Trigger (every minute) that claims due Job rows from the database matches the glossary's Job model, and the same code can be called by a timer on Node.

### 3.5 Queues (for fan-out)

- Message size 128 KB; max consumer batch 100; `sendBatch` max 100 messages (or 256 KB); retries up to 100; consumer wall time 15 min; up to 250 concurrent consumer invocations; retention up to 14 days (24 h on Free). https://developers.cloudflare.com/queues/platform/limits/

### 3.6 CPU and subrequest limits for a 300-Student announcement

- Subrequests per invocation: 50 (Free) / 10,000 (Paid). Calls to D1, R2 and KV count too. https://developers.cloudflare.com/workers/platform/limits/#subrequests
- Max simultaneous outgoing connections: 6 per invocation; the seventh queues until a response header arrives. https://developers.cloudflare.com/workers/platform/limits/#simultaneous-open-connections
- D1 queries per invocation: 50 (Free) / 1,000 (Paid). A `db.batch()` counts as one query. https://developers.cloudflare.com/d1/platform/limits/
- LINE: Multicast takes 500 user IDs, max 5 message objects per request, 200 requests per second. 300 Students is **one** subrequest. https://developers.line.biz/en/reference/messaging-api/#send-multicast-message
- Telegram: no multicast. 300 `sendMessage` calls = 300 subrequests, over the Free limit of 50 and within the Paid 10,000. At about 30 messages per second, sending takes at least 10 s of wall time. https://core.telegram.org/bots/faq#my-bot-is-hitting-limits-how-do-i-avoid-this
- **INFERENCE:** On Free, split a Telegram send into Jobs of roughly 40 recipients each so one invocation stays below 50 subrequests (leave room for D1 reads/writes). On Paid one Job can send all 300, but still throttle to about 25-30 per second. Pick the chunk size from configuration, not code.

### Implications for the spec (Workers vs Node)

Declare **Workers Paid as the baseline** for any Deployment on Cloudflare: Free's 10 ms CPU and 50 subrequests rule out password hashing, email OTP via an arbitrary-recipient API, and a Telegram announcement in one go. Store the password-hash algorithm and parameters with every hash; do not hardcode Argon2. Put email behind a sender interface with a Cloudflare/HTTP adapter and an SMTP adapter. The Kernel "backup" feature should use D1's REST export (note: it blocks the database while it runs, so schedule it off-hours) plus Time Travel for rollback, and should not rely on FTS5. Jobs must be small, chunked and idempotent.

---

## 4. Rust on Cloudflare Workers and self-host (รัสต์บน Workers และติดตั้งเอง)

Added after the owner switched the Core's language from TypeScript to Rust (instruction from the coordinator, 2026-11-05). It replaces the earlier Drizzle and Hono/HTMX/pnpm/changesets sections; those no longer apply. Crate versions are `max_stable_version` from the crates.io API on 2026-11-05. "Compile probe" means we built a tiny crate for `wasm32-unknown-unknown` on rustc 1.89.0 in a scratch directory; that proves it compiles, not that it runs on Workers.

> Consequence for the repo: ADR-0002 (Hono, Drizzle), ADR-0004 (Hono JSX + HTMX) and ADR-0005 (`@student-care/*` on npm) describe the TypeScript stack, and sections 2.2 and 3.1 above use TypeScript snippets. They need superseding or rewriting; this file does not touch them.

### 4.1 workers-rs (crate `worker`)

- Current version is `worker` **0.8.7** (published 2026-09-25), with `worker-sys` and `worker-macros` at the same version. https://crates.io/crates/worker and https://github.com/cloudflare/workers-rs
- Minimum Rust version is **1.91**. Our local rustc 1.89.0 refused to build it ("worker@0.8.7 requires rustc 1.91"). https://crates.io/api/v1/crates/worker/0.8.7
- Cloudflare documents `workers-rs` as the supported way to write Workers in Rust. The Rust toolchain needs the `wasm32-unknown-unknown` target; the project is scaffolded with `cargo generate cloudflare/workers-rs`. https://developers.cloudflare.com/workers/languages/rust/
- Entry points are set with the `#[event(...)]` macro: `fetch` (HTTP), `scheduled` (**Cron Triggers**), `queue` (Queues, needs the `queue` feature) and `start`. https://developers.cloudflare.com/workers/languages/rust/
- A `ScheduledEvent` exposes the cron expression (`cron()`), the event type and the scheduled time, and `ScheduleContext::wait_until` extends the run. https://github.com/cloudflare/workers-rs/blob/main/worker/src/schedule.rs
- **Bindings** come from `Env`: `Secret` (set with `wrangler secret put`), `Var` (from `wrangler.toml`), `KvStore`, `Bucket` (**R2**), `D1Database` (**D1**), `Queue`, `ObjectNamespace` (Durable Objects), `Fetcher` (service binding), `RateLimiter`, `SecretStore`. https://developers.cloudflare.com/workers/languages/rust/
- Secrets are read with `env.secret("NAME")` (or `ctx.secret(...)` inside the `Router`). https://github.com/cloudflare/workers-rs#durable-object-kv-secret--variable-bindings
- **D1** is behind the `d1` Cargo feature. `D1Database` offers `prepare(sql)`, `batch(statements)`, `exec(sql)`, `dump()` and `with_session(...)`. A prepared statement has `bind`, `first`, `run`, `all`, `raw`. Results are deserialised with serde. https://github.com/cloudflare/workers-rs/blob/main/worker/src/d1/mod.rs
- D1 inside a Worker is a **binding** (an in-platform call), not an HTTP client. The HTTP route is Cloudflare's separate REST API (used for export in 3.3). https://developers.cloudflare.com/d1/worker-api/d1-database/
- **R2** is the `Bucket` binding (module `worker::r2`). The S3-API route in ADR-0002 is a different path (HTTP with SigV4), and is not part of `worker`. https://github.com/cloudflare/workers-rs/tree/main/worker/src/r2
- `worker` has a Cloudflare email binding: `env.send_email("EMAIL")` with `SendEmailBuilder` (text and HTML), and a `send-email` example that builds MIME with `mail-builder`. https://github.com/cloudflare/workers-rs/tree/main/examples/send-email
- Outgoing HTTP is `worker::Fetch` / `Fetcher::fetch_request`; with the `http` feature the request and response types are the `http` crate's. https://github.com/cloudflare/workers-rs#http-feature
- Workers RPC support is **experimental**; the WIT code generator is "pre-alpha, with no support guarantee". Avoid both in the Core. https://github.com/cloudflare/workers-rs#rpc-support
- `worker::signals::is_near_cpu_limit()` lets a Job stop gracefully before the CPU limit (relevant to the chunked sends in 3.6). https://github.com/cloudflare/workers-rs#cpu-limits
- Rust async runs as one JavaScript Promise per handler through `wasm-bindgen-futures`; there is no multithreaded runtime. https://developers.cloudflare.com/workers/languages/rust/#async-wasm-bindgen-futures
- An experimental `tokio` / emscripten mode (`worker-build --emscripten`, feature `experimental_tokio`) exists; treat it as non-baseline. https://github.com/cloudflare/workers-rs#emscripten

### 4.2 axum / tower inside workers-rs

- **Yes.** The `http` feature (and an `axum` feature) makes the `fetch` handler take `http::Request<worker::Body>` and return `http::Response<B>`, "much easier to use crates which use these standard types such as `axum` and `hyper`". https://github.com/cloudflare/workers-rs#http-feature
- The official `axum` example builds an `axum::Router`, then calls `router.call(req).await` through `tower_service::Service` inside `#[event(fetch)]`. It uses `axum 0.8` with `default-features = false, features = ["json"]`. https://github.com/cloudflare/workers-rs/tree/main/examples/axum
- That example's state is held in `Arc<T>` and uses the KV binding, so axum extractors and `State` work. https://github.com/cloudflare/workers-rs/blob/main/examples/axum/src/lib.rs
- Disabling axum's default features matters: the defaults pull in the Tokio/hyper server pieces that target native sockets. **INFERENCE** from the example's feature list; not stated as a rule by axum.
- `worker` re-exports a `#[worker::send]` macro to make non-`Send` JavaScript-backed futures usable where axum requires `Send`. The macro exists, but we did not verify a case that needs it. https://github.com/cloudflare/workers-rs/blob/main/worker/src/lib.rs
- Versions: `axum` 0.8.9 (MSRV 1.80), `tower` 0.5.3. https://crates.io/crates/axum and https://crates.io/crates/tower
- **UNCONFIRMED by our own build:** we could not compile `worker 0.8.7` locally. Our default rustc is 1.89 and the newer installed toolchain (1.95) has no wasm32 target; we did not install one. The compile claim therefore rests on the official example, not on our test.
- **INFERENCE (self-host):** the same axum `Router` (without `worker`) can run under `axum::serve` on Tokio for the self-hosted binary, provided handlers do not use Workers-only types. Keep Workers types in the adapter layer (ADR-0002 "no Cloudflare API outside the adapter").

### 4.3 Wasm size, startup and cold start

- Worker size limit is **64 MiB uncompressed** on both Free and Paid; "There is no compressed size limit." https://developers.cloudflare.com/workers/platform/limits/#worker-size
- Startup limit is **1 second** to parse and run global scope; a failure shows as error `10021` "Script startup exceeded CPU time limit". Larger bundles raise startup time. https://developers.cloudflare.com/workers/platform/limits/#worker-startup-time
- Cloudflare says Wasm Workers "are typically larger than an equivalent Worker written in JavaScript" and recommends `wasm-opt`. https://developers.cloudflare.com/workers/runtime-apis/webassembly/#binary-size
- The official template sets `[profile.release] lto = true, strip = true, codegen-units = 1`, and `worker-build` runs `wasm-opt` automatically. https://developers.cloudflare.com/workers/languages/rust/#binary-size-wasm-opt
- Cold starts: isolates "start very quickly"; Cloudflare states the isolate model "eliminates the cold starts of the virtual machine model". This is a statement about isolates in general, not a measurement for Rust Wasm. https://developers.cloudflare.com/workers/reference/how-workers-works/
- **UNCONFIRMED:** the actual size and startup time of a realistic Core (axum + serde + templates + crypto) as Wasm. No primary source publishes one. We only know the 64 MiB / 1 s limits. Measure with the first slice (`wrangler check startup` generates a CPU profile). https://developers.cloudflare.com/workers/platform/limits/#worker-startup-time
- Probe size note: our own probe of `argon2` alone produced a 37.6 KB wasm (without `wasm-opt`, one exported function), which only shows that a single crate is small, not that the whole Core will be.

### 4.4 SQLite access for the self-hosted binary, and one query layer for both

Versions on 2026-11-05: `rusqlite` 0.41.1 (MSRV 1.89), `sqlx` 0.9.0 (MSRV 1.94), `libsql` 0.10.0. https://crates.io/crates/rusqlite , https://crates.io/crates/sqlx , https://crates.io/crates/libsql

- **rusqlite**: bindings to the C SQLite library. The `bundled` feature compiles SQLite in. A feature `ffi-sqlite-wasm-rs` switches to the `sqlite-wasm-rs` crate on `wasm32-unknown-unknown`. https://github.com/rusqlite/rusqlite/blob/master/README.md
- **sqlx**: async, runtime-agnostic (Tokio, async-std) with an SQLite driver that "uses the libsqlite3 C library" and "directly invokes the SQLite3 API via `libsqlite3-sys`". It also has an `Any` driver chosen by URL at runtime, and compile-time-checked queries. https://github.com/launchbadge/sqlx
- **libsql** (Turso): its Cargo features include `core` (embedded), `replication`, `remote` (HTTP/Hrana), and `wasm` / `cloudflare` (which depend on `worker`). The wasm/Cloudflare path talks to a **remote libSQL server**, it does not open D1. https://github.com/tursodatabase/libsql/blob/main/libsql/Cargo.toml
- **D1 from Rust does not expose a raw SQLite handle.** `sqlx` with the `sqlite` feature cannot be used inside a Worker: "`sqlx-sqlite` contains a native dependency" and "actual D1 itself doesn't expose raw database interface". https://github.com/ohkami-rs/sqlx-d1#background
- **Community (not Cloudflare) crates map sqlx onto the D1 binding:**
  - `sqlx-d1` 0.4.3 (2026-08-22), compatible with `worker 0.8`. Compile-time SQL checking uses Miniflare's local D1 file. Unsupported: **transactions**, connection pool, derive of `Type`/`Encode`/`Decode`. https://github.com/ohkami-rs/sqlx-d1
  - `sqlx-cloudflare-d1` 0.2.0 (2026-10-05): an `sqlx::Database` over the Workers D1 binding. Its README says D1 has "no interactive transactions", so atomic work goes through a batch, and integers are limited to ±2^53 − 1 "since values cross as JavaScript numbers". https://github.com/sqlx-contrib/sqlx-cloudflare
  - Both are small third-party projects; check maintenance before depending on them. https://crates.io/api/v1/crates?q=cloudflare%20d1
- The D1 integer limit is stated by a third-party README. We did not find it in Cloudflare's D1 pages. **UNCONFIRMED** by Cloudflare; treat IDs above 2^53 as unsafe on D1 and use UUID or text IDs. https://github.com/sqlx-contrib/sqlx-cloudflare
- D1 batched statements are SQL transactions ("it aborts or rolls back the entire sequence"), which is the atomic primitive available from Rust through `D1Database::batch`. https://developers.cloudflare.com/d1/worker-api/d1-database/
- **Answer: one query layer for both?** Not with a single off-the-shelf driver that is confirmed:
  - Option A (**our own trait**): a small `Db` trait in the Core (`query`, `execute`, `batch`) with two implementations: D1 over `worker::d1` on Workers, and `rusqlite` (or sqlx-sqlite) on the self-hosted binary. SQL strings are shared because both are SQLite. Works today, costs us the adapter. **INFERENCE** (no source calls this out).
  - Option B (`sqlx-d1` on Workers + `sqlx` SQLite on self-host): same sqlx query API on both. Limited by no transactions on D1 and by third-party maintenance. Not tested by us.
  - Option C (`libsql` everywhere): one driver, but on Workers it reaches a remote libSQL server, so it replaces D1 rather than targeting it. That contradicts ADR-0002's D1 on Cloudflare.
- Both D1 and local SQLite need only a SQL subset (no Postgres features), as ADR-0002 already requires. Atomic multi-statement work should be expressed as a **batch** so it works on D1. https://developers.cloudflare.com/d1/worker-api/d1-database/

### 4.5 Server-side HTML templating in Wasm, with HTMX

- **askama** 0.16.1 (MSRV 1.88): templates are "compiled into your crate", generating type-safe Rust at compile time. Compile probe: a one-field template built for `wasm32-unknown-unknown`. https://github.com/askama-rs/askama
- **maud** 0.27.0: `html!` macro that "compiles your markup to specialized Rust code". Compile probe for `wasm32-unknown-unknown` succeeded. Last release 2025-02-02. https://github.com/lambda-fairy/maud
- **minijinja** 3.0.5 (MSRV 1.70): runtime templates; its README lists "Compiles to WebAssembly". Compile probe for `wasm32-unknown-unknown` succeeded. https://github.com/mitsuhiko/minijinja
- None of the three requires JavaScript or a client build step, so they fit ADR-0004's goal (Modules add pages by registering routes that return HTML). **INFERENCE.**
- **HTMX is independent of the language.** It is a JavaScript file plus `hx-*` attributes in the HTML the server returns, so any of the three crates can emit it. CSRF: htmx documents sending a token with `hx-headers` on `<html>`/`<body>`; checking it is the backend's job. https://htmx.org/docs/#csrf-prevention
- Trade-off (**INFERENCE**): compile-time templates (askama, maud) catch template errors at build time and add no runtime engine, but a Module's templates are fixed at build time. minijinja renders templates loaded at runtime, which suits per-Deployment text changes, but then they must be bundled into the Worker or stored in D1/R2, because Workers expose only a read-only virtual file system holding the bundle plus a temporary `/tmp`. https://developers.cloudflare.com/workers/runtime-apis/nodejs/fs/
- **UNCONFIRMED:** an official axum integration for each templating crate on Workers. We did not check the `axum` integration features of askama or maud; returning `String` as `Html` from an axum handler needs no integration crate. **INFERENCE.**

### 4.6 Password hashing in Wasm

- The RustCrypto `argon2` crate (0.6.0, MSRV 1.85) is "Pure Rust" and supports `no_std`, including environments without `alloc`. https://github.com/RustCrypto/password-hashes/blob/master/argon2/README.md
- **Compile probe, default features: FAILS** on `wasm32-unknown-unknown`, in `getrandom`, because that target has no default randomness source. https://github.com/rust-random/getrandom#webassembly-support
- **Compile probe, two fixes both PASS:** (a) `argon2 = { version = "0.6", default-features = false, features = ["alloc"] }` (no random-salt helper; salt supplied by the caller); (b) default features plus `getrandom = { version = "0.4", features = ["wasm_js"] }` and `RUSTFLAGS='--cfg getrandom_backend="wasm_js"'`, which uses `Crypto.getRandomValues` through `wasm-bindgen`. https://github.com/rust-random/getrandom#webassembly-support
- We **did not run** Argon2 on Workers. Compiling is not a CPU, memory or time measurement. Memory-hard hashing must fit the 128 MB isolate memory, and CPU time is 10 ms on Free and 30 s by default on Paid (see 3.1). **UNCONFIRMED:** that Argon2id with recommended parameters runs inside those limits. https://developers.cloudflare.com/workers/platform/limits/#cpu-time
- **PBKDF2**: the pure-Rust `pbkdf2` crate (0.13.0) compiled for `wasm32-unknown-unknown` with `sha2 0.11` (compile probe). It runs in Wasm, so the iteration cap in 3.1 (which applies to the platform's WebCrypto) does not apply to it, but the CPU time does. https://github.com/RustCrypto/password-hashes/tree/master/pbkdf2
- **PBKDF2 through WebCrypto from Rust** is possible: `web-sys` defines `SubtleCrypto`, `CryptoKey` and `Pbkdf2Params`; `worker` re-exports `web_sys`. Native WebCrypto avoids Wasm CPU cost but inherits the platform's iteration cap (3.1). **INFERENCE** that it works end-to-end; we did not run it. https://github.com/wasm-bindgen/wasm-bindgen/blob/main/crates/web-sys/Cargo.toml
- `scrypt` 0.12.0 also exists in the same RustCrypto repository (not compile-probed by us). https://crates.io/crates/scrypt
- **INFERENCE (design):** keep the hash algorithm and parameters inside the stored hash string (PHC format, which the RustCrypto `password-hash` API uses), so a Deployment on Node can hash with Argon2id and a Free-plan Workers Deployment can use a cheaper setting, and both can verify each other's records.

### 4.7 Sending email from Rust on Workers

- Two routes work from `worker`: (1) the Cloudflare Email Service binding through `env.send_email(...)` (Beta; Workers Paid for arbitrary recipients; see 3.2) and (2) any HTTPS email API called with `worker::Fetch`. https://github.com/cloudflare/workers-rs/tree/main/examples/send-email and https://developers.cloudflare.com/email-service/platform/pricing/
- Resend's send endpoint takes `from`, `to`, `subject` and an HTML/text body as JSON over HTTPS with a bearer API key. Plain `Fetch` with `serde_json` is enough; no SMTP is involved. https://resend.com/docs/api-reference/emails/send-email
- SMTP crates such as `lettre` (0.11.23) need TCP; Workers blocks port 25 and Cloudflare IP ranges (3.2), so use the HTTP route on Workers and SMTP only in the self-hosted binary. https://developers.cloudflare.com/workers/runtime-apis/tcp-sockets/ and https://crates.io/crates/lettre
- **INFERENCE (design):** an `EmailSender` trait in the Core with `CloudflareEmail`, `HttpApiEmail` (Resend, Postmark and similar) and `Smtp` implementations, chosen per Deployment by configuration, as in 3.2.
- **UNCONFIRMED:** whether `reqwest` (0.13.5) is a good choice inside Workers; the official examples use `worker::Fetch`, so prefer that. https://github.com/cloudflare/workers-rs

### 4.8 Publishing a family of crates on crates.io

- crates.io has **no scopes**. Names are global: "allocated on a first-come-first-serve basis. Once a crate name is taken, it cannot be used for another crate." https://doc.rust-lang.org/cargo/reference/publishing.html
- A publish is "generally permanent. The version can never be overwritten, and the code cannot be deleted." `cargo yank` only blocks new dependents. So choose names before the first publish. https://doc.rust-lang.org/cargo/reference/publishing.html
- Namespaces are an accepted idea but not shipped as far as we can verify: RFC 3243 "Packages as (optional) namespaces" is merged, and its tracking issue rust-lang/rust#122349 was still **open** on 2026-11-05. **UNCONFIRMED** whether crates.io accepts `student_care::core` style names today. https://github.com/rust-lang/rfcs/blob/master/text/3243-packages-as-optional-namespaces.md and https://github.com/rust-lang/rust/issues/122349
- Convention without scopes is a shared prefix and hyphens, for example `student-care-core`, `student-care-line`, `student-care-telegram`. This is a convention, not a rule from crates.io. The manifest should fill `license`, `description`, `repository`, `readme`. https://doc.rust-lang.org/cargo/reference/publishing.html
- **Name check on 2026-11-05 (crates.io is public, so a `404` means unregistered, unlike an npm scope):** `student-care`, `student-care-core`, `student-care-kernel`, `student-care-line`, `student-care-telegram`, `student-care-channel` and `campus-care` all returned `404` (unregistered). `scp-core` is taken. A search for `student-care` returned 0 crates. A `404` can change at any time; reserve by publishing. https://crates.io/api/v1/crates/student-care-core
- Cargo can publish a workspace natively: Cargo 1.90 "Stabilize multi-package publishing ... even if they have inter-dependencies", for example `cargo publish --workspace` or `cargo publish -p foo -p bar`. Publishing is "still non-atomic": a server error can leave the workspace partly published. https://doc.rust-lang.org/cargo/CHANGELOG.html and https://doc.rust-lang.org/cargo/commands/cargo-publish.html
- A Cargo changelog entry says a regression in 1.96 made workspace publish fail with a false deadlock error while waiting for registry confirmation; it was fixed. Pin the Cargo version in CI. https://doc.rust-lang.org/cargo/CHANGELOG.html
- **release-plz** 0.4.3: opens a "Release PR" with version bumps and changelogs from Conventional Commits and `cargo-semver-checks`, then on merge tags `<package_name>-v<version>`, creates a GitHub release and runs `cargo publish`. It handles workspaces. https://github.com/release-plz/release-plz
- **cargo-release** 1.1.7: supports workspaces "using cargo's native flags, like `--workspace`, `--exclude` and `--package`", updates dependents' versions, and has no PR step. https://github.com/crate-ci/cargo-release
- Local customer repos can depend on a Core checkout by `path` or `git` (ADR-0001's `file:`/`pnpm link` equivalent), and pin a published version before delivery. **INFERENCE** from Cargo path dependencies; not verified in this research.
- Customer-specific crates must stay out of this public workspace (ADR-0005). The public/private split is unchanged by the language.

### Implications for the spec (Rust)

Rust fits the Core, and the platform gives us what the first slice needs: `worker` 0.8.7 exposes D1, R2, Queues, Cron (`#[event(scheduled)]`), secrets and an email binding, and axum 0.8 runs inside it. Three decisions must now be made explicitly. First, a **database trait** owned by the Core, because no confirmed driver targets both D1 and local SQLite; D1 has no interactive transactions, so the trait should offer a `batch` and no `begin`. Second, **password hashing** parameters and algorithm stored per hash, because Argon2 compiles for Wasm only with a getrandom fix and nobody has shown it fits Workers' CPU and memory limits. Third, **crate names**: reserve a `student-care-*` family early, because crates.io has no scopes and a publish is permanent. The spec should also add a measurement task: build the first slice for Wasm, record binary size and startup time against the 64 MiB and 1 s limits, and run Argon2id on a deployed Worker. The Rust toolchain floor is 1.91 for `worker` 0.8.7 and 1.94 for `sqlx` 0.9.

---

## Questions that could not be confirmed

1. LINE: maximum size of a file/image/video a Student can send (1.1).
2. LINE: whether Replies still work after the monthly Push quota is exhausted, and when the monthly count resets (1.2).
3. Telegram: maximum number of inline keyboard buttons (2.1); download limit for files a Student sends (2.5).
4. Telegram: behaviour of editing a bot message older than 48 hours, and the "not modified" error (2.5).
5. Workers: numeric cap on PBKDF2 iterations and scrypt cost (3.1); whether Cron Triggers are exactly-once (3.4); Cloudflare Email Service (Beta) suitability for the customer (3.2).
6. Rust: a local compile of `worker` 0.8.7 (needs rustc 1.91 plus the wasm32 target; not available here) (4.2).
7. Rust: real Wasm size and startup time of a realistic Core, and whether Argon2id fits Workers' CPU and memory limits (4.3, 4.6).
8. Rust: D1's integer precision limit as a Cloudflare statement (only a third-party README says ±2^53 − 1) (4.4); maintenance of the third-party `sqlx-d1` and `sqlx-cloudflare-d1` crates.
9. Rust: whether crates.io accepts namespaced crate names today (RFC 3243 tracking issue still open) (4.8).
