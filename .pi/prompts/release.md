---
description: Cut a release — checks, changelog, version bump, tag, push
---

Perform a full release of the agent-sandbox project. Follow these steps exactly, stopping and reporting any failure before continuing.

## 1. Clean working tree

Run `git status --porcelain`. If there are any uncommitted changes, stop and tell the user what is dirty. Do not proceed until the tree is clean.

## 2. Pre-flight checks

Run in order — stop on any failure:
1. `cargo fmt --check` — fail if formatting is off (tell user to run `cargo fmt`)
2. `cargo clippy -p ags -- -D warnings` — fail on any warnings
3. `cargo test -p ags` — all tests must pass

## 3. Find commits since last release

Run `git describe --tags --abbrev=0 2>/dev/null` to find the last tag.
- If no tag exists, treat all commits as unreleased.
- Run `git log <last_tag>..HEAD --oneline` (or `git log --oneline` if no prior tag).
- If there are **no commits** since the last tag, stop and tell the user there is nothing to release.

Show the user the list of commits.

## 4. Determine version bump

Read the current version from `crates/ags/Cargo.toml` (`package.version`).

Analyse the commits and suggest a bump type, following semver:
- **patch** — bug fixes, docs, refactors, chores with no API or behaviour change
- **minor** — new features, additive changes
- **major** — breaking changes

Tell the user the suggested bump and the resulting new version, then ask them to confirm or choose a different bump type (major / minor / patch).

## 5. Update CHANGELOG.md

Edit (or create) `CHANGELOG.md` at the repo root.

- Prepend a new section at the top (below any existing header):
  ```
  ## [vX.Y.Z] — YYYY-MM-DD
  ```
- Group the commits into sections if possible: **Features**, **Bug Fixes**, **Chores / Other**.
- Each entry is a bullet: `- <commit subject> (<short hash>)`
- Keep all existing content intact below the new section.

## 6. Bump version

Edit `package.version` in `crates/ags/Cargo.toml` to the new version string (without the `v` prefix).

Then run `cargo check -p ags`.

## 7. Commit, tag, push

```bash
git add crates/ags/Cargo.toml Cargo.lock CHANGELOG.md
git commit -m "release: vX.Y.Z"
git tag vX.Y.Z
git push origin HEAD
git push origin vX.Y.Z
```

Show the user the tag and confirm the push succeeded. Remind them that GitHub Actions will now build and publish release artifacts automatically.
