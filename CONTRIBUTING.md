# Contributing

Thanks for helping. harpseal is a Core that customers build their own
Deployments on, so changes are small, reviewed one at a time, and never
carry a real customer's name or data.

## How work is organised

- **One pull request per ticket.** Work starts from a GitHub issue labelled
  `ready-for-agent` or `ready-for-human` (see `docs/agents/`). Branch
  `feat/<issue>-<slug>` (or `fix/…`) from `main`; never push to `main`.
- **Conventional Commits** for the pull request title: `feat(kernel): …`,
  `fix(channel-line): …`, `docs: …`, `ci: …`. Pull requests are
  squash-merged, so the title becomes the commit release-plz reads for
  versions and changelogs.
- **Fictional content only.** Examples, fixtures and tests use Seal Bay
  University (มหาวิทยาลัยอ่าวแมวน้ำ) and invented Thai names.

## Definition of done

`just check` passes. It runs the same five commands as CI:

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo check --target wasm32-unknown-unknown -p harpseal -p harpseal-channel-line \
  -p harpseal-channel-telegram -p harpseal-channel-fake -p harpseal-runtime-cloudflare \
  -p harpseal-example-worker
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps
```

Also:

- a test for every new behaviour, at the Deployment seam where possible
  (`harpseal-testing`); the Channel contract suite stays green;
- `just e2e` passes when the change touches the chat flow, announcements or
  the roster import;
- no `unsafe`, no platform feature flags in the Core or channel crates,
  files of at most 400 lines, `missing_docs` clean;
- `GLOSSARY.md`, `docs/adr/` and `docs/guide/` updated when a term, a
  decision or a documented behaviour changes (say so in the pull request).

## Licence

By contributing you agree that your contribution is licensed under the
Apache License 2.0, as the project is (`LICENSE`).
