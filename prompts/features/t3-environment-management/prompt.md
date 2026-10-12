# Global T3 environment management

The user approved implementation of the lifecycle-management changes discussed
in this session. These requirements supersede the original current-directory
default for public T3 management commands.

- `ags t3 status` lists all registered environments without starting them.
- Include a stable environment identifier, repository, SSH alias, owner/server
  and container state, runtime, and per-environment errors.
- `ags t3 stop [environment]` and `ags t3 upgrade [environment]` accept an
  optional identifier copied from status. Retain `--repository` as an explicit
  single-repository selector, independent of the command's working directory.
- Global upgrade presents a checkbox list of all working instances so users
  choose which environments to upgrade and interrupt. Nothing is preselected.
- Global stop uses the same selection flow. Cancel or an empty selection changes
  nothing. Unhealthy and stopped registrations remain visible in status and are
  excluded from the global action list.
- Without a terminal, require a specific environment/repository or explicit
  `--all` automation opt-in. `--all` acts on all working environments.
- Execute selected actions sequentially, preserve mounted data and SSH identity,
  report each outcome, continue after individual failures, and exit nonzero if
  any selected action fails.
- Maintain scoped help/completions and regression coverage. Follow the repository
  size policy, validate changes, commit, and push `feat/t3-integration`.
