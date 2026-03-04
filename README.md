# pi-sandbox

`ags` is a Rust-based sandbox launcher for AI agents using rootless Podman.

## Security-first note

Use limited-scope tokens only:
- least privilege
- short expiration
- dedicated bot/machine credentials when possible
- rotate and revoke quickly on suspicion

## Quick start

From repo root:

```bash
# one-time setup (keys + sandbox bootstrap)
make setup

# health checks
make doctor

# run pi in sandbox
make run
```

Equivalent direct command (without Makefile):

```bash
cargo run -p ags -- --agent pi
```

## Commands

Using `make` convenience targets:

- `make setup`
- `make doctor`
- `make update`
- `make update-agents`
- `make run`
- `make run-browser`
- `make install`
- `make uninstall`

Or directly via CLI:

```bash
cargo run -p ags -- setup
cargo run -p ags -- doctor
cargo run -p ags -- update
cargo run -p ags -- update-agents
cargo run -p ags -- --agent pi
cargo run -p ags -- --agent claude
cargo run -p ags -- --agent shell
```

Pass args through with `--`:

```bash
cargo run -p ags -- --agent pi -- --continue
```

## Config

Default config path:

- `~/.config/ags/config.toml`

If missing, `ags` creates a default config on first run.

Use `config/config.example.toml` as a reference template.

## Notes

- Container base image is Fedora (`config/Containerfile`).
- Python stays installed in the image intentionally for agent/tool scripting workflows.
