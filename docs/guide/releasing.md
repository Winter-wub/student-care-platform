# Releasing

How crates reach crates.io. Back to the
[README](https://github.com/Winter-wub/student-care-platform#readme).

## The flow

1. Pull requests are squash-merged with a Conventional Commit title.
2. On every push to `main`, the `release-plz` workflow opens (or updates)
   the **Release PR**: version bumps and changelog entries computed from
   those titles (`release-plz.toml`).
3. Merging the Release PR, and only that (`release_always = false`), makes
   the next workflow run tag `<crate>-v<version>`, create a GitHub release
   and publish the changed crates to crates.io. Publishing cannot be
   undone: a version on crates.io can be yanked, never deleted.

Until the `CARGO_REGISTRY_TOKEN` secret exists, the release job only prints
that it skipped; nothing is tagged or published.

`just release-dry` shows what the next Release PR would hold (`release-plz
update` in a throwaway worktree) and packages every crate (`cargo publish
--workspace --dry-run`). It needs the network and publishes nothing.

## Owner setup

- **Crate names.** Reserve on crates.io: `harpseal`, `harpseal-channel-line`,
  `harpseal-channel-telegram`, `harpseal-channel-fake`,
  `harpseal-runtime-cloudflare`, `harpseal-runtime-native`, `harpseal-cli`
  and `harpseal-testing`.
- **`CARGO_REGISTRY_TOKEN`** repository secret: a crates.io API token
  allowed to publish those crates.
- **Repository setting** Settings → Actions → General → "Allow GitHub
  Actions to create and approve pull requests" must be on, or the Release PR
  cannot be opened.
- **CI on the Release PR.** A PR opened with the workflow's `GITHUB_TOKEN`
  does not trigger other workflows, so CI does not run on it. Either accept
  that and run the checks by hand (`just check`, or re-run the CI workflow
  from the Actions tab on that branch) before merging, or add a
  `RELEASE_PLZ_TOKEN` secret (a fine-grained PAT or a GitHub App token with
  contents and pull-requests write): the workflow then opens the Release PR
  with it and CI runs as on any PR.
