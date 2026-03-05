# Architecture Overview

`ags` follows a simple pipeline:

1. Parse CLI args.
2. Load + validate config.
3. Prepare assets/secrets/ssh/git metadata.
4. Build a launch plan.
5. Render plan into `podman run` args.
6. Execute container.

---

## Main modules

- `cli.rs`
  - Defines command enums and parses args.
- `config/*`
  - TOML deserialization + validation into strongly typed config.
- `cmd/*`
  - Subcommand implementations (`setup`, `doctor`, `update`, etc).
- `agent.rs`
  - Agent-specific profiles (command, mounts, env, browser integration).
- `plan/*`
  - Converts config + runtime state into final `LaunchPlan`.
- `podman/*`
  - Turns `LaunchPlan` into `podman run` arguments and executes.
- `ssh.rs`
  - Dedicated ssh-agent lifecycle + key loading.
- `secrets.rs`
  - Multi-source secret resolution.
- `assets.rs`
  - Writes embedded Containerfile/guard/settings.

---

## Execution model

### Run mode

- User calls `ags --agent <name> ...`.
- Config is validated.
- Secrets are resolved and written to an env file.
- Mounts and env are assembled per agent profile.
- Container entrypoint script runs chosen agent command.

### Subcommands

- `setup`/`doctor`/`update`/`update-agents` operate as host-side utilities.
- `install` writes embedded assets and optional self-link.

---

## Key design constraints

- Rootless Podman execution.
- Principle of least privilege for mounts and env.
- Reproducible defaults via embedded assets.
- Config-driven behavior with validation before launch.
- Agent state persisted on host volumes.
