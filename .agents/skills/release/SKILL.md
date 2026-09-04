---
name: release
description: User-invoked release workflow for agent-sandbox. Use ONLY when the user explicitly invokes /release; never invoke this skill automatically.
user-invocable: true
disable-model-invocation: true
---

# Release Workflow

This skill is manual-only. It MUST run only after the user explicitly invokes `/release`. An agent MUST NEVER invoke it automatically, infer that it should run from release-related conversation, or invoke it from another skill or workflow.

Perform a full release of the agent-sandbox project. Follow these steps exactly, stopping and reporting any failure before continuing.

## 1. Clean Working Tree

Run `git status --porcelain`. If there are any uncommitted changes, stop and tell the user what is dirty. Do not proceed until the tree is clean.

## 2. Pre-flight Checks

Run these commands in order and stop on any failure:

1. `cargo fmt --check` - fail if formatting is off and tell the user to run `cargo fmt`.
2. `cargo clippy -p ags -- -D warnings` - fail on any warning.
3. `cargo test -p ags` - all tests must pass.

## 3. Find Changes Since the Last Release

Run `git describe --tags --abbrev=0 2>/dev/null` to find the last tag.

- If no tag exists, treat all commits as unreleased.
- Run `git log <last_tag>..HEAD --oneline` or `git log --oneline` if no prior tag exists.
- If there are no commits since the last tag, stop and tell the user there is nothing to release.

Show the user the commits. Use the commit list as source material only, not as changelog text. Inspect the relevant diff and, when necessary, the changed code or documentation to understand the user-visible outcome of the work.

## 4. Confirm the Version

Read the current version from `crates/ags/Cargo.toml` (`package.version`).

Analyze the changes and suggest a SemVer bump:

- **patch** - backward-compatible bug fixes or changes with no new user-facing capability.
- **minor** - backward-compatible new functionality.
- **major** - breaking changes.

Tell the user the suggested bump and resulting version, then ask them to confirm or choose `major`, `minor`, or `patch`. Stop and wait for their answer before continuing.

## 5. Draft and Confirm the Changelog

Read `CHANGELOG.md`, then draft the exact new release section without editing the file.

The new section MUST follow [Keep a Changelog 1.0.0](https://keepachangelog.com/en/1.0.0/):

- Use the heading `## [X.Y.Z] - YYYY-MM-DD`, without a `v` prefix and with an ISO 8601 date.
- Group entries under only the applicable standard headings: `Added`, `Changed`, `Deprecated`, `Removed`, `Fixed`, and `Security`.
- Omit empty headings.
- Include notable changes only.
- Write for users: describe the capability gained, behavior changed, problem fixed, migration required, or security impact.
- Consolidate related commits into one clear entry when they produce one user-visible outcome.
- Do not paste, lightly rewrite, or mechanically enumerate the Git log. Do not include commit hashes merely to mirror commit history.

Historical content is immutable for this workflow. Existing changelog entries MUST NOT be renamed, reordered, reformatted, rewritten, or otherwise updated to match Keep a Changelog. Do not retrofit the existing header, introduction, release headings, category names, dates, or links. Only the newly proposed release section must use the new format.

Show the complete proposed section exactly as it would be written and ask the user to confirm it. Make clear that `CHANGELOG.md` has not been edited yet. Stop and wait for explicit approval.

If the user requests changes, revise the draft, show the complete revised section, and ask for confirmation again. Do not edit `CHANGELOG.md`, bump the version, commit, tag, or push until the user explicitly approves the changelog text.

## 6. Write the Changelog and Bump the Version

After explicit changelog approval:

1. Insert the confirmed section at the top of the release history, after an existing `Unreleased` section if present or otherwise after the existing changelog title and introduction.
2. Preserve all historical changelog content exactly as it was.
3. Edit `package.version` in `crates/ags/Cargo.toml` to the confirmed version without the `v` prefix.
4. Run `cargo check -p ags`.
5. Review `git diff -- CHANGELOG.md crates/ags/Cargo.toml Cargo.lock`. Verify that the changelog contains exactly the confirmed new section and no historical content changed. Stop if it differs.

## 7. Commit, Tag, and Push

Run:

```bash
git add crates/ags/Cargo.toml Cargo.lock CHANGELOG.md
git commit -m "release: vX.Y.Z"
git tag vX.Y.Z
git push origin HEAD
git push origin vX.Y.Z
```

Show the user the tag and confirm the push succeeded. Remind them that GitHub Actions will now build and publish release artifacts automatically.
