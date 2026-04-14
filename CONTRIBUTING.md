# Contributing

Thanks for considering a contribution.

This project is a Rust CLI (`ags`) for running coding agents inside a Podman sandbox.

## Development prerequisites

- Rust toolchain
- Podman
- git
- bash

Optional:

- make

---

## Local development

From repository root:

```bash
# build
cargo build -p ags

# format
cargo fmt

# lint
cargo clippy -p ags -- -D warnings

# tests
cargo test -p ags
```

Useful run examples:

```bash
cargo run -p ags -- --help
cargo run -p ags -- doctor
cargo run -p ags -- --agent shell
```

AUR package install smoke test on a clean Arch container:

```bash
podman run --rm -it \
  -v "$PWD:/src:Z" \
  archlinux:latest \
  bash -lc '
    set -euxo pipefail
    pacman-key --init
    pacman-key --populate archlinux
    pacman -Sy --noconfirm archlinux-keyring
    pacman -Syyu --noconfirm
    pacman -S --noconfirm --needed base-devel sudo rsync
    useradd -m build
    echo "build ALL=(ALL) NOPASSWD: ALL" >> /etc/sudoers
    rsync -a --delete /src/ /tmp/build/
    chown -R build:build /tmp/build
    sudo -u build bash -lc "cd /tmp/build && ./scripts/render-pkgbuild.sh && makepkg -s -f --noconfirm"
    pkg=$(find /tmp/build -maxdepth 1 -type f | grep -E "/agent-sandbox-.*-x86_64\.pkg\.tar\.zst$" | grep -v -- "-debug-" | head -n1)
    echo "PKG=$pkg"
    test -n "$pkg"
    pacman -U --noconfirm "$pkg"
    command -v ags git ssh-keygen ssh-add podman
    ags --help
  '
```

---

## Project structure (quick map)

- `crates/ags/src/main.rs` — CLI entrypoint + command dispatch
- `crates/ags/src/cli.rs` — argument parsing and help text
- `crates/ags/src/cmd/` — subcommands (`setup`, `doctor`, `update`, ...)
- `crates/ags/src/config/` — config schema, parsing, validation
- `crates/ags/src/plan/` — launch plan assembly (mounts/env/security/entrypoint)
- `crates/ags/src/podman/` — podman args + execution
- `crates/ags/src/assets.rs` — embedded assets writer
- `config/` — containerfile + config template
- `agent/` — embedded guard extension and settings example

---

## Pull request guidelines

1. Keep PRs focused and small.
2. Include tests for behavior changes.
3. Keep CLI help text and README/docs in sync.
4. Prefer explicit, user-actionable error messages.
5. Maintain security-conscious defaults.

### Before opening PR

- [ ] `cargo fmt`
- [ ] `cargo clippy -p ags -- -D warnings`
- [ ] `cargo test -p ags`
- [ ] docs updated (`README.md`, `docs/*`, config examples) if needed

---

## Source file size policy

- Rust implementation files have a hard limit of **500 lines**.
- If a file is approaching the limit, split it before adding more behavior.
- Keep tests out of the implementation file whenever possible.
  - Prefer integration tests under `crates/ags/tests/`.
  - For module-private coverage, prefer sibling `*_tests.rs` files instead of inline `#[cfg(test)] mod tests` blocks in the implementation file.
- When touching an oversized file, treat reduction/splitting as part of the work instead of growing it further.

---

## Documentation expectations

If behavior changes, update relevant docs:

- User-facing workflows: `README.md`
- Command behavior: `docs/COMMANDS.md`
- Config schema/semantics: `docs/CONFIG.md`
- Common failures/fixes: `docs/TROUBLESHOOTING.md`

---

## Security guidance for contributors

- Do not add real secrets/tokens to repo or docs.
- Keep examples placeholder-only.
- Favor least-privilege mounts and env passthrough.
- Avoid expanding host access by default without strong justification.
