# Profession-Centric Tool Picker

Redesign `ags tools` around purposeful executable tools rather than DNF packages.

## Requirements

- A tool is something purposefully executed to reach a goal. Libraries, headers,
  certificate bundles, terminal metadata, and auxiliary packages are not tools.
- Keep the public `--packages` option and `config/tool-packages.example.json` filename.
- Use exactly three horizontal profession tabs: General, Software Development,
  and Operations and DevOps.
- Tabs are views only. Switching tabs must not apply a preset.
- Within each tab, group tools under visible subcategory divider rows.
- Define each tool once. A tool may appear in several professions or areas, but
  every appearance must share one selection state and emit its DNF packages once.
- Keep ordinary Unix utilities, AGS runtimes, `dbus-devel`, and `sqlite-devel`
  in a fixed, non-selectable image baseline.
- Keep `socat`, OpenSSH clients, tmux, and Wayland clipboard as purposeful tools
  and place them in the relevant profession areas.
- Treat `kitty-terminfo` as a hidden dependency of tmux.
- Reintroduce a required `default` flag on every tool definition.
- Default tools: Git, GitHub CLI, OpenSSH clients, fd, ripgrep, rsync, make,
  pkg-config, sccache, socat, and tmux.
- Put all selectable language tools under Languages in both Software Development
  and Operations and DevOps.
- Put npm and Python pip under Package Managers in both Software Development and
  Operations and DevOps.
- Put Wayland clipboard in both Software Development and Operations and DevOps,
  as well as the General desktop area.
- Do not show DNF package names in the primary UI.
- Existing explicit configuration remains authoritative. Saving through the
  picker removes fixed baseline names from `extra_dnf_packages` while preserving
  unknown non-baseline packages.
- An explicit empty `extra_dnf_packages` list builds the fixed baseline only.
