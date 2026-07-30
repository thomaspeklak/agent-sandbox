# Walkthrough

Host command secret resolution is implemented in `crates/ags/src/secrets.rs` and validated in `crates/ags/src/config/parse.rs`. The resulting secrets are passed to sandboxed agents, including OpenCode MCP processes.

The sandbox image is defined by `config/Containerfile`. OpenCode stores state below `/home/dev/.local/share/opencode`, configured by `crates/ags/src/agent.rs`. The image pre-creates `/home/dev/.local/share` while assigning the dev user ownership so runtime child mounts do not leave the shared parent root-owned.
