# Deploying to Cloudflare Workers

Back to the [README](https://github.com/Winter-wub/student-care-platform#readme).

Workers Paid is required (Free's 10 ms CPU fails logins and imports). The
build needs `worker-build` (`cargo install worker-build`) and binaryen's
`wasm-opt` (`brew install binaryen`); `wrangler.toml`'s `[build]` command runs
`worker-build --profile wasm-release --no-opt`, then `wasm-opt -Oz`.

Locally, with no Cloudflare account (D1 is simulated by miniflare):

```sh
cd deployments/example/worker
cp .dev.vars.example .dev.vars                  # git-ignored; placeholders
npx wrangler@4 dev --local                       # http://localhost:8787/notice
curl 'http://localhost:8787/__scheduled?cron=*+*+*+*+*'   # with --test-scheduled: runs one tick
```

The first request in an isolate builds the Deployment and applies its
migrations to D1; later requests reuse it. A Cron Trigger (`* * * * *`, UTC)
calls `Deployment::tick` at the trigger's scheduled time. `job.max_attempts`
counts retries: the default 3 means up to 4 runs, retried after 1, 5 and 30
minutes. A Job whose runtime died mid-run (CPU limit, trap) is re-claimed when
its 15-minute lock expires, while it has retries left, and is marked failed
(`lock expired`) otherwise.

Webhooks are answered 200 once the call is verified and its events are
claimed; the handling runs after the response under `ctx.wait_until` (#97),
so pass the `Context` of `#[event(fetch)]` to
`harpseal_runtime_cloudflare::fetch`. That work still counts against the
invocation's CPU limit and `waitUntil` window (about 30 seconds after the
response). An event for a chat account whose earlier event this isolate is
still handling waits for it, under its own request's `wait_until`. Time
spent waiting eats into LINE's reply token (about a minute), so a backlog
turns Replies into metered Pushes (Quota). Handling cut off by a limit
keeps an unfinished claim, which the next Cron tick logs as `inbound.lost`.

A preview on your own account:

```sh
cd deployments/example/worker
npx wrangler@4 d1 create harpseal-example            # paste database_id into wrangler.toml
npx wrangler@4 secret put FAKE_SECRET                 # required
npx wrangler@4 secret put RESEND_API_KEY              # optional: without it every email send fails
npx wrangler@4 secret put LINE_CHANNEL_SECRET         # and LINE_ACCESS_TOKEN, LINE_LIFF_ID,
                                                      # TELEGRAM_BOT_TOKEN, TELEGRAM_WEBHOOK_SECRET,
                                                      # TELEGRAM_WEB_APP_URL when the Channels are set up
npx wrangler@4 r2 bucket create harpseal-example-files   # optional: then uncomment [[r2_buckets]]
npx wrangler@4 deploy
curl https://harpseal-example.<subdomain>.workers.dev/notice
```

Bindings the runtime reads (`CloudflareSettings`): D1 `DB`, R2 `FILES`
(optional: without it file operations answer an error naming the binding),
secret `RESEND_API_KEY`, var `EMAIL_FROM` (set in `wrangler.toml`).
`Date.now()` is frozen while a request computes, so never measure durations
with the Clock there; use `wrangler tail`.
