# Feature Parity Checklist (Current `pis` → `ags --agent pi`)

## Core launcher parity
- [x] Config resolution + strict validation
- [x] Ensure image build if missing
- [x] Ensure sandbox git signing config
- [x] Dedicated SSH agent management
- [x] Secret precedence parity (first successful source wins)
- [x] Passthrough env parity
- [x] Browser startup parity
- [x] Wayland clipboard mount parity
- [x] Mount logic parity (`optional`, `create`, `kind`, `when`)
- [x] Podman security flags parity
- [x] Guard read/write roots env parity
- [x] Container boot dirs parity
- [x] Browser network/port-forward parity
- [x] External git metadata mount parity (worktrees)

## Companion command parity
- [x] `setup` key generation + optional secret-store fill
- [x] `doctor` checks + summary + exit codes
- [x] `update` args/behavior parity
- [x] `install/uninstall` symlink/bootstrap parity

## Acceptance scenarios
- [ ] Normal git repo
- [ ] Worktree with `.git` file to external root
- [ ] Browser mode on/off
- [ ] Optional mounts/tools missing
- [ ] Required mount missing failure path
- [ ] Secret from env + secret-tool fallback
- [ ] Signed commit + push path
