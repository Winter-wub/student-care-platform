# Testing

The in-process harness, the Channel contract suite and the e2e walk. Back
to the [README](../../README.md).

## The harness

```rust
let h = harpseal_testing::Harness::new();     // in-memory SQLite, fake email/HTTP/clock/random
let app = h.start().await;                    // build + migrate + Router
assert_eq!(app.get("/notice").await.status, 200);
app.fake_webhook([inbound]).await;            // signed Fake Channel webhook (text, file, Action, follow, blocked, unblocked)
h.fake_channel().seen_by(&account);           // what the Student saw, with Delivery and Metering
app.count("SELECT count(*) FROM outbound_message").await;  // rows a later request reads back
h.clock().advance_minutes(5);                 // time only moves when told
h.emails();  h.http_requests();               // captured, never sent
```

`Harness::new().with_fake_profile(FakeProfile::LineLike)` makes the Fake
Channel imitate LINE (one reply token covers 5 objects, Push metered, LINE's
degrade limits, no attachments); `FakeProfile::TelegramLike` has no reply
tokens, meters nothing and sends one message per Card.
`h.fake_channel().fail_next_batch_at(k)` makes the next send stop with
`RateLimited` before its object `k`, and a Fake event with `event_id` set
travels on the wire, so tests can deliver it twice.

The Channel contract suite lives in `harpseal_testing::contract`: an adapter
crate adds `harpseal-testing` as a dev-dependency and calls
`contract::run(|| target)` with its Channel, recorded webhooks (`Fixtures`) and
a `Wire` reading its `RecordingHttp`. It runs green against the Fake Channel in
every profile and against the LINE and Telegram adapters.

Runtime port tests live in `crates/harpseal-runtime-native/tests` (SQLite
batch rollback, local files, a mock SMTP server, Resend and S3 signing against
the recording HttpClient, the ticker). The real-S3 round trip runs only when
`HARPSEAL_TEST_S3_BUCKET`, `HARPSEAL_TEST_S3_ENDPOINT`, `AWS_ACCESS_KEY_ID` and
`AWS_SECRET_ACCESS_KEY` are set.

Back-office pages: `h.create_staff(email, password, &["Viewer"]).await`, then
`let b = app.browser(); b.login(email, password).await;` keeps cookies and the
page's CSRF token (`b.get(..)`, `b.post(path, form)` adds `_csrf`,
`b.post_raw(..)` does not).

Use `h.builder()` to add Modules before booting, or pass any Deployment to
`h.boot(d)` (the example's tests do `example_deployment::builder(h.services(),
..).channel(h.fake_channel())`).

## `just e2e`

`deployments/example/e2e` (`harpseal-example-e2e`, not published) tests the
real binaries over real HTTP: it creates a temporary directory and SQLite
file, picks a free port, generates `FAKE_SECRET`, runs `harpseal staff
create --admin` (reading the temporary password from its output) and starts
`harpseal-example-native` in dev mode. Then it walks: Staff sign-in and the
forced password change, the 50-row roster as CSV (`full`) and again as
XLSX (`upsert`, nothing changes), a Fake Student from follow through
Consent, student id, the emailed OTP and the Link (Menu unlocked), an
announcement needing acknowledgement sent to year 1, a tick delivering its
Card, the ack in chat twice ("รับทราบแล้ว" the second time), the Mini App
showing it acknowledged, `/admin/usage`, `/admin/security`, `/admin/audit`,
and the self-unlink from the Menu (Link revoked, Menu locked). It prints one
row per step with its time, exits 1 on any failure (with the server log's
tail) and kills the server.

What the Student saw comes from the dev routes (`/dev/outbox`,
`/dev/email`, `/dev/tick`, see [self-hosting](self-hosting.md#dev-mode-harpseal_dev_routes1));
database checks read the SQLite file read-only. It lives beside the
example rather than in `harpseal-testing`, which is published and runs
in-process. It is not part of CI: run it before merging changes to the chat
flow, announcements or the import, and before every release (or start the
`CI` workflow by hand on GitHub with `e2e` ticked).
