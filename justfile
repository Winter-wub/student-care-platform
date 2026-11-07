# harpseal tasks; `just --list` shows them. Every recipe echoes its
# commands. None needs the network except `release-dry` (and the first
# `cargo` run that downloads crates).

# The crates that must build for Cloudflare Workers (wasm32).
wasm_crates := "-p harpseal -p harpseal-channel-line -p harpseal-channel-telegram -p harpseal-channel-fake -p harpseal-runtime-cloudflare -p harpseal-example-worker"

# `just run` and `just seed` share these (the server and the CLI must see
# the same database). Override any of them from the environment.
# A local SQLite file (git-ignored: *.sqlite3).
export HARPSEAL_DB := env_var_or_default("HARPSEAL_DB", "harpseal.sqlite3")
# Where uploaded roster files go (git-ignored).
export HARPSEAL_FILES := env_var_or_default("HARPSEAL_FILES", ".harpseal-files")
# Dummy Mini App ids: the example refuses to start without them; real ones
# come from LINE (LIFF) and BotFather (docs/guide/channels.md).
export LINE_LIFF_ID := env_var_or_default("LINE_LIFF_ID", "0000000000-devDummy")
export TELEGRAM_WEB_APP_URL := env_var_or_default("TELEGRAM_WEB_APP_URL", "https://dev.sealbay.invalid/tg")

# List the recipes.
default:
    @just --list

# The definition-of-done checks: fmt, clippy, tests, wasm32, docs (as CI).
check:
    cargo fmt --all --check
    cargo clippy --workspace --all-targets -- -D warnings
    cargo test --workspace
    cargo check --target wasm32-unknown-unknown {{wasm_crates}}
    RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps

# Same as `check`: what CI runs on every pull request.
ci: check

# Format every crate.
fmt:
    cargo fmt --all

# Serve the example on 127.0.0.1:8080 in dev mode (needs a FAKE_SECRET of 16+ characters).
run:
    @test -n "${FAKE_SECRET:-}" || { echo 'set FAKE_SECRET first (16+ characters, not change-me): FAKE_SECRET=$(openssl rand -hex 16) just run'; exit 1; }
    HARPSEAL_DEV_ROUTES=1 cargo run -p harpseal-example-native

# Demo data: first Administrator, 50 Students, 4 announcements, 2 texts (`--force` to reseed; demo databases only).
seed *args:
    #!/usr/bin/env bash
    set -euo pipefail
    cli="cargo run -q -p harpseal-example-native --bin harpseal --"
    echo "+ $cli migrate"
    $cli migrate
    # Only "Staff already exist" (a re-seed) is fine; any other failure stops here.
    echo "+ $cli staff create --admin --email admin@sealbay.example"
    if ! out="$(cargo run -q -p harpseal-example-native --bin harpseal -- staff create --admin --email admin@sealbay.example 2>&1)"; then
        if grep -q "Staff already exist" <<<"$out"; then
            echo "(Staff exist already: seeding as the oldest Administrator)"
        else
            echo "$out" >&2
            exit 1
        fi
    else
        echo "$out"
    fi
    echo "+ $cli seed-example {{args}}"
    $cli seed-example {{args}}

# Back up the database and its files into a new directory (stop `just run` first).
backup dir:
    cargo run -q -p harpseal-example-native --bin harpseal -- backup {{dir}}

# Restore a backup into an empty database (`--force` deletes the rows of the backed-up tables first).
restore dir *args:
    cargo run -q -p harpseal-example-native --bin harpseal -- restore {{dir}} {{args}}

# Boot the example on a free port and walk the first slice over HTTP (not in CI).
e2e:
    cargo build -p harpseal-example-native --bins
    cargo run -q -p harpseal-example-e2e -- "${CARGO_TARGET_DIR:-target}/debug"

# What the next Release PR would hold, and a publish check of every crate (network; publishes nothing).
release-dry:
    #!/usr/bin/env bash
    # release-plz 0.4 has no `release-pr --dry-run`: run `release-plz update`
    # (versions and changelogs the Release PR would carry) in a throwaway
    # worktree of HEAD, then package and verify every publishable crate.
    set -euxo pipefail
    tmp="$(mktemp -d)"
    git worktree add --detach "$tmp" HEAD
    trap 'git worktree remove --force "$tmp"' EXIT
    cd "$tmp"
    release-plz update
    git status --short
    git --no-pager diff --stat
    cargo publish --workspace --dry-run
