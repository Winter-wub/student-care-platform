# Prototype: back office + Mini App on Workers with axum + maud + HTMX (issue #5)

Throwaway. Measures what ADR-0006 left open, on a real deployed Worker, and gives the owner two pages to judge on a
phone. Built on `prototype/message-render` (Design A). Measured 2026-11-05 (UTC) with rustc 1.99.1, `worker` 0.8.7,
worker-build 0.8.7, wasm-bindgen 0.2.130, wrangler 4.158.0, binaryen 133.

**Preview:** <https://harpseal-proto-example.englishwithgift.workers.dev> (`/admin/login` with
`officer@sealbay.example` / `seal-bay-proto-2026`, `/app?student=68010001`, `/notice`).

> **The Cloudflare account is on Workers Free, not Paid.** `wrangler deploy` with `[limits] cpu_ms = 30000` fails:
> "CPU limits are not supported for the Free plan" (code 100328). So the preview runs under Free's 10 ms CPU limit
> and some requests (login, `/bench/*`) end in `503` / error 1102 once Free's burst allowance runs out. Every CPU
> number below is measured, so the Paid verdicts compare those numbers with Paid's 30 s limit. Nothing ran on Paid.

## What was built

- `crates/harpseal-runtime-cloudflare`: `services(&Env, "DB") -> Services`. `D1Db` (`query` via
  `stmt.raw({columnNames: true})`, `execute` via `run()` meta, `batch` via `D1Database::batch`), `WorkerHttp` over
  `worker::Fetch`, `WorkerClock` (`Date.now`), `WorkerRandom` (getrandom 0.4 `wasm_js`; the feature alone was enough,
  no `--cfg`), `LogEmail` (logs), `R2Files` (`todo!()`). JS futures are wrapped in `worker::send::SendFuture`
  (no `unsafe` in our code).
- `crates/harpseal`: `password` (Argon2id / PBKDF2 → PHC string, `verify` dispatches on the PHC algorithm),
  `ui` (one maud layout with `Shell::BackOffice | MiniApp`, `ui.css` served at `/static/app.css`, HTMX 2.0.4 pinned
  from jsDelivr), `ModuleRegistrar::public(router)` for routes outside `/admin`.
- `deployments/example/deployment/src/pages`: a `Pages` Module with Staff login (session cookie `HttpOnly; Secure;
  SameSite=Lax; Path=/admin`, token SHA-256 in `staff_session`), a `Staff` extractor as the session guard (redirects,
  or `HX-Redirect` for HTMX), `/admin/students` (50 seeded Students, `hx-get` search with 250 ms debounce), an
  announcement form (title, body, audience dropdown with live count, one atomic `batch` writing `announcement` +
  `announcement_text`, flash), `/app` (Link status + 5 newest announcements with an `hx-post` "รับทราบ" toggle) and
  `/notice`. Migrations `sql/0002_pages.sql`, `sql/0003_seed.sql` (fictional Seal Bay University).
- `deployments/example/worker`: cdylib, `#[event(fetch)]` → the Deployment's axum router (cached per isolate in a
  `thread_local`) merged with `/bench/*`. `deployments/example/native` (`harpseal-example-native`) runs the same pages
  on rusqlite: `cargo run -p harpseal-example-native` → <http://127.0.0.1:8080>.

Prototype gaps (deliberate): no lockout after failed logins, no dummy hash for unknown emails, no CSRF token
(SameSite=Lax only), no sliding session update, the Mini App reads the Student from `?student=`/a cookie instead of a
LIFF id token / Telegram `initData` App Session, `announcement_ack` stands in for `announcement_delivery.acked_at`.

## Setup commands

```sh
npx wrangler@4 d1 create harpseal-proto                     # id 2a809205-9ca6-4b45-ae79-8c522225d6a6, region APAC
cd deployments/example/worker
for f in ../deployment/sql/0001_hello.sql ../deployment/sql/0002_pages.sql ../deployment/sql/0003_seed.sql sql/bench.sql; do
  npx wrangler@4 d1 execute harpseal-proto --remote --file=$f -y; done
npx wrangler@4 deploy                                       # runs worker-build --release
npx wrangler@4 tail harpseal-proto-example --format json > tail.jsonl   # cpuTime / wallTime per request
```

## a. Wasm size

`[profile.release]`: `opt-level = "z"`, `lto = true`, `codegen-units = 1`, `panic = "abort"`, `strip = true`.

| Artifact | Bytes | gzip -9 |
|---|---:|---:|
| `cargo build` output `target/wasm32-unknown-unknown/release/example_worker.wasm` (before wasm-bindgen) | 1 629 735 | – |
| `worker-build --release --no-opt` → `build/index_bg.wasm` | 926 107 | 320 185 |
| `worker-build --release` (default: worker-build's own wasm-opt 132 with `-O`) | 813 712 | 328 995 |
| `wasm-opt -Oz` (binaryen 133) on the `--no-opt` output | 801 819 | 328 902 |
| **Deployed**: `worker-build --release` with `wasm-opt = ["-Oz", …]` in `[package.metadata.wasm-pack.profile.release]` | **803 580** | 329 421 |
| wrangler at deploy (wasm + 27.3 KB `index.js` + shim) | **Total Upload: 831.20 KiB** | **gzip: 333.44 KiB** |
| Same with `--features xlsx` (calamine) | 1 263 416 | 611 849 (wrangler: 1280.26 KiB / 610.30 KiB) |

- wasm-opt saves 12–13 % raw. The gzip size barely changes (it grows by about 9 KB), and gzip is what goes over the network.
- worker-build defaults to `-O`; `-Oz` saves another 10 KB. The explicit args need the feature flags
  (`--enable-exception-handling` etc.), otherwise binaryen rejects the module ("Tags require exception-handling").
- wasm-bindgen warns: "the `target_features` custom section is missing, likely due to `strip = true` … Avoid
  `strip = true` and run `wasm-opt` on the wasm-bindgen output instead." Harmless here (worker-build runs wasm-opt
  after bindgen), but drop `strip = true` from the real profile.
- Limit is 64 MiB uncompressed: we use 1.3 %.

## b. Startup

```sh
npx wrangler@4 deploy        # "Worker Startup Time: 7 ms" (first deploy), then 4 ms on the next three deploys
npx wrangler@4 check startup
```

```
│ Bundle: 831.20 KiB / gzip: 331.54 KiB
│ Local startup profile:
│   Profile window: 14.5 ms
│   Sampled time: 8.8 ms
│   Active: 5.1 ms (including 0.0 ms garbage collection)
│   Idle: 3.7 ms
│   Samples: 2
```

Limit is 1 s. With calamine the deploy also reported 4 ms.

## c. Password hashing on the Worker

`GET /bench/hash?algo=argon2id|pbkdf2|pbkdf2-webcrypto&n=&m=&i=` hashes `seal-bay-proto-2026` with a fixed salt.
**`Date.now()` is useless here:** it read 0 ms around every CPU loop, and even after awaiting two D1 round trips the
"before → after I/O minus I/O" estimate stayed at 0–3 ms for work that tail measured at 54–856 ms. So the clock
does not move during CPU work, and the I/O that follows does not bring the elapsed CPU time back either. The numbers
below are `cpuTime` from `wrangler tail --format json`; only requests that finished with `outcome: ok` are counted.

| Scheme | CPU per hash (tail `cpuTime`) | Notes |
|---|---|---|
| Argon2id m=19 MiB, t=2, p=1 (OWASP) | **54, 59 ms** (n=1); 261 ms for n=5 → 52 ms/hash | Real login `POST /admin/login`: 51 ms CPU, 131 ms wall |
| Argon2id m=64 MiB, t=2, p=1 | 148, 293 ms (n=1); 589 ms for n=3 → ~196 ms/hash | Fits the 128 MB isolate |
| PBKDF2-HMAC-SHA256 600 000 (`pbkdf2` + `sha2`, in wasm) | **586, 856 ms** | ~11–16× Argon2id-19M |
| PBKDF2 600 000 via `crypto.subtle.deriveBits` | **refused** | `Pbkdf2 failed: iteration counts above 100000 are not supported (requested 600000).` |
| PBKDF2 100 000 via WebCrypto | 17, 20, 24 ms | **Counts as Worker CPU** (it is not free); 600 k would be ~120 ms if allowed |

- **Workers Paid (30 s):** no scheme comes close. The worst case (Rust PBKDF2, 856 ms) is under 3 % of the limit.
- **Workers Free (10 ms), observed:** every scheme is over 10 ms. Requests succeed while Free's burst allowance
  lasts, then end as `exceededCpu` ("Worker exceeded CPU time limit.", HTTP 503 / error 1102) at 10–20 ms CPU. In 12
  back-to-back single-hash calls, 7 failed. On Free a Staff login would fail at random.
- **Recommendation for `staff.password_hash`: Argon2id, m=19456 KiB, t=2, p=1**, PHC string
  `$argon2id$v=19$m=19456,t=2,p=1$<salt b64>$<hash b64>` (16-byte salt from `Services::random`, 32-byte output).
  ~55 ms CPU per login on Workers Paid. PBKDF2 is ~10× slower in wasm, and the WebCrypto route cannot reach the
  OWASP 600 k. `harpseal::password::verify` reads the algorithm from the PHC, so the parameters can be raised later
  without a migration (re-hash on the next successful login).

## d. Request profile: `/admin/students` with 50 rows

Logged in once, redeployed (new version, so the first request lands on a fresh isolate), then 5 × `GET /admin/students`
2 s apart from Thailand. Each request does 2 D1 queries (session check, list) and renders the full page.

| # | tail `cpuTime` | tail `wallTime` | client `time_total` |
|---|---:|---:|---:|
| 1 (cold isolate) | 38 ms | 115 ms | 0.39 s |
| 2 | 4 ms | 79 ms | 0.28 s |
| 3 | 6 ms | 88 ms | 0.26 s |
| 4 | 4 ms | 93 ms | 0.34 s |
| 5 (another isolate, likely cold) | 23 ms | 99 ms | 0.24 s |

Warm requests cost 4–6 ms CPU, so even Free's 10 ms holds for page views. Wall time is D1 round trips (D1 primary in
APAC, served from SIN). The cold cost (~35 ms) is wasm instantiation plus building the Deployment and router once.

## e. XLSX (`calamine` 0.38) on wasm32

- **calamine has no `xlsx` feature.** Its features are `default = []`, `dates`/`chrono` and `picture`; every
  format (xlsx/xls/xlsb/ods) is always compiled. `calamine = { version = "0.38", default-features = false }` is the
  minimum.
- `cargo check --target wasm32-unknown-unknown -p example-worker --features xlsx`: **passes**. clippy on wasm32 with
  the feature also passes.
- At runtime on the Worker: `POST /bench/xlsx` with a 500-row roster (7 Thai headers from #8, 18 KB file) returned
  `{"rows":501,"cols":7,…}` with the Thai headers intact. 7–84 ms CPU (84 on a cold isolate).
- **Size delta: +459 836 bytes raw (+57 %), +276.9 KiB gzip at deploy** (831 → 1280 KiB, gzip 333 → 610 KiB). Startup
  stayed 4 ms. Most of it is zip/deflate + quick-xml + encoding tables for all formats.

## f. D1 batch of 500 upserts

`POST /bench/batch?n=500&fail=0|1`: one `Db::batch` of 500 `INSERT … ON CONFLICT (student_code) DO UPDATE` into
`bench_roster`. With `fail=1` the 500th statement binds NULL into a NOT NULL column.

| Run | `batch_ms` (Date.now around the awaited batch) | tail cpu / wall | Result |
|---|---:|---|---|
| fail=0, first (500 inserts) | 206 ms | 26 / 274 ms | 500 ok |
| fail=0, again (500 updates) | 305 ms | 26 / 370 ms | 500 ok |
| fail=0, again | 189 ms | 17 / 247 ms | 500 ok |
| fail=1 | 147 ms | 13 / 196 ms | error `NOT NULL constraint failed: bench_roster.display_name: SQLITE_CONSTRAINT`, **0 rows of that run written** |
| fail=1 | 149 ms | 6 / 193 ms | same, 0 rows |

`SELECT substr(student_code,1,1), count(*) FROM bench_roster GROUP BY 1` afterwards → only `B | 500`: no row
with the failing run's `F` prefix exists. **D1 `batch` is all-or-nothing**, as `Db::batch` promises. (`Date.now`
works here because the time is spent waiting on I/O, not on CPU.)

## Checks

```
cargo fmt --check                                   → exit 0
cargo clippy --workspace -- -D warnings             → Finished `dev` profile … (exit 0; also with --all-targets)
cargo test --workspace                              → exit 0: 28 passed, 0 failed, 1 ignored (suites of 10, 9, 5, 4; 17 empty)
cargo check --target wasm32-unknown-unknown -p harpseal -p harpseal-channel-line -p harpseal-channel-telegram \
  -p harpseal-channel-fake -p harpseal-runtime-cloudflare   → Finished `dev` profile … (exit 0)
cargo clippy -p example-worker --target wasm32-unknown-unknown --features xlsx -- -D warnings → exit 0
```

## Verdict

- **axum + maud + HTMX on Workers: acceptable, keep it.** 0.8 MB wasm (333 KiB gzip), startup 4–7 ms out of 1 s, a
  warm page costs 4–6 ms CPU, and wall time is set by D1 round trips (~80–115 ms for two queries). The same Module
  code runs unchanged on the native binary. At 360 px wide the pages have no horizontal scroll, Thai wraps cleanly,
  and tap targets are 48 px (checked in headless Chrome inside 360 px frames; not yet seen inside the LINE/Telegram
  webviews, which is the owner's check).
- **Hashing: Argon2id m=19 MiB, t=2, p=1, PHC string** (~55 ms CPU per login). Not PBKDF2: in wasm it is slower, and
  WebCrypto is capped at 100 k iterations and still counts as CPU.
- **500-row batch: fine.** 0.2–0.3 s wall, ~20 ms CPU, atomic. The roster import's 500-row chunks can stay.
- **Painful, but not enough to revisit the stack:**
  1. Workers Paid is required, not just the baseline. On Free, logins and the roster import fail at random. This
     account is on Free today.
  2. In-Worker timing is impossible (`Date.now` frozen, even across I/O). Performance work needs `wrangler tail`
     or Workers Observability. Do not build features on `Clock` deltas.
  3. calamine costs +460 KB raw / +277 KiB gzip for one upload page. That is acceptable under the limits, but a
     CSV-only roster on Workers (XLSX converted in the browser or by Staff) would keep the bundle at 0.8 MB.
  4. workers-rs gaps we had to paper over: D1 `raw({columnNames})` is not exposed (we call it through `Reflect`),
     every JS future needs `SendFuture`, worker-build defaults to `wasm-opt -O`, and `strip = true` triggers a
     wasm-bindgen warning. `panic = "abort"` means a panic kills the request with no message, so add
     `console_error_panic_hook` or keep handlers panic-free.

## Questions for the owner (look & feel)

1. Student list on a phone: cards (name, code, programme, year, as now) or a denser table that scrolls? Which
   fields must be visible without opening a Student (Link status per Channel, Tags, email)?
2. Branding: the university's colours and logo in the bar and Mini App, or a neutral harpseal theme? Keep the
   system Thai font (no download, as now), or bundle IBM Plex Sans Thai / Noto Sans Thai (~100 KB+) for the same look
   on every phone?
3. Mini App: should "รับทราบ" be one-way (the #10 design) or undoable (the prototype toggles)? Show the full body in
   the list (as now) or titles only, with a detail page?

## Cloudflare resources created (delete when done)

| Resource | Name / id | Delete |
|---|---|---|
| Worker (workers.dev route) | `harpseal-proto-example` → `harpseal-proto-example.englishwithgift.workers.dev` | `npx wrangler@4 delete harpseal-proto-example` |
| D1 database | `harpseal-proto` (`2a809205-9ca6-4b45-ae79-8c522225d6a6`, APAC) | `npx wrangler@4 d1 delete harpseal-proto` |

Nothing else (no R2 bucket, no secrets, no routes on a zone, no plan change).
