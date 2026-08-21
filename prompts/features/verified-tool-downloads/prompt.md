# Verified Tool Downloads

Extend the profession-centric sandbox tool picker with Terraform, OpenShift CLI
(`oc`), Ansible Playbook, kubectl, AWS CLI, Helm, `dig`, `hcloud`, `uv`, and the
Python Black formatter.

## Requirements

- Keep `curl` in the fixed image baseline rather than exposing it as a selectable
  tool.
- Mark every newly selectable tool as optional (`default: false`).
- Use Fedora 44 DNF packages where available: `ansible-core`,
  `kubernetes-client`, `awscli2`, `helm`, `bind-utils`, `hcloud`, `uv`, and
  `black`.
- Put infrastructure tools under Infrastructure automation, Kubernetes and
  OpenShift tools under Containers and orchestration, cloud-vendor CLIs under
  Cloud platforms, `dig` under Network, `uv` under Package managers, and Black
  under Software Development / Code quality.
- Support tools that are unavailable from standard Fedora repositories through
  downloads from authoritative vendor sources.
- Keep download source metadata in the tool catalog, including pinned versions,
  architecture-specific HTTPS URLs, SHA-256 digests, archive formats, and exact
  executable members.
- Never execute downloaded installer scripts or accept unverified downloads.
- Preserve selected download metadata as reproducible build input so image builds
  do not depend on the original catalog still being available.
- Apply selected downloads to explicit `update-image` builds and automatic image
  builds.
