# Python Tools in AGS

## Root cause

`[[tool]]` is currently a single-file bind mount, not a package or runtime mechanism.

`crates/ags/src/config/parse.rs` translates this:

```toml
[[tool]]
path = "/usr/bin/ansible-lint"
container_path = "/usr/local/bin/ansible-lint"
```

into one Podman volume mount. The exposed `ansible-lint` script contains:

```python
#! /usr/bin/python3
from ansiblelint.__main__ import _run_cli_entrypoint
```

Inside AGS, `/usr/bin/python3` is the sandbox's Fedora Python 3.14. Its
site-packages do not contain `ansiblelint`, `ansible`, `yaml`, `rich`, or the
other host-installed dependencies, which produces:

```text
ModuleNotFoundError: No module named 'ansiblelint'
```

This is not specific to Ansible. Python, Node, Ruby, and dynamically linked
tools can all fail when the mounted executable depends on files that are absent
from the sandbox image. The mounted `uv` executable works because it is a
sufficiently compatible native executable.

## Options

| Option | Benefits | Problems | Best use |
| --- | --- | --- | --- |
| Mount the host Python environment | Smallest conceptual change | Virtual environments are non-portable; Python versions, absolute shebangs, native extensions, libc, and system libraries must match | Temporary workaround on identical host and container distributions |
| Use a project-local `.venv` | Works today; built against sandbox Python; workspace persists | One environment per repository; project setup required | Projects already managing Python development dependencies |
| Use `uvx` with a persistent cache | One cache mount; isolated environments; no dependency mounting or image rebuild | First execution needs network; command uses `uvx` unless wrapped | Best immediate solution |
| Add an AGS-managed `uv tool` store | Transparent commands; persistent and isolated; installed against the exact sandbox image | Requires a new AGS installation and update flow | Best long-term AGS feature |
| Bake tools into the image | Immutable, reproducible, and immediately available | Image rebuild and bloat; less flexible versions | Standard tools needed in every sandbox |
| Execute tools on the host through a bridge | Uses the existing host installation exactly | Major sandbox escape surface; complicated cwd, environment, TTY, and signal handling | Generally inappropriate |
| Use PEX, Nix, or another self-contained bundle | Can fit the current single-file or bundle model | Requires repackaging each tool; native dependencies can remain difficult | Organizations already producing such artifacts |

## Immediate approach

Expose `uv` and persist its sandbox-side cache:

```toml
[[tool]]
name = "uv"
path = "~/.local/bin/uv"
container_path = "/usr/local/bin/uv"
mode = "ro"

[[tool.directory]]
host = "~/.cache/ags/uv"
container = "/home/dev/.cache/uv"
mode = "rw"
kind = "dir"
create = true
```

Run a pinned tool inside the sandbox:

```bash
uvx --from 'ansible-lint==26.8.0' ansible-lint
```

All Python dependencies are installed into `/home/dev/.cache/uv` using the
sandbox's Python and ABI. Only that cache directory must be mounted. Current
`ansible-lint` releases support Python 3.14.

A wrapper can preserve the normal command name:

```sh
#!/bin/sh
exec uvx --from 'ansible-lint==26.8.0' ansible-lint "$@"
```

AGS could eventually generate this wrapper automatically.

## Long-term design

Extend `[[tool]]` to distinguish host binaries from sandbox-native Python tools:

```toml
[[tool]]
name = "ansible-lint"
python_package = "ansible-lint==26.8.0"
```

Add an explicit command such as `ags update-tools` that:

1. Starts a temporary container from the configured AGS image.
2. Installs each Python tool using `uv tool install`.
3. Stores isolated environments below `cache_dir`.
4. Verifies the expected executables.
5. Mounts the completed tool store read-only during normal runs.
6. Adds its `bin` directory to the sandbox `PATH`.
7. Recreates environments when the image or Python version changes.

Installation should be explicit rather than automatic during launch because
installing Python packages can execute third-party build code.

The `tools-packages` branch's host `apt` and `dnf` installation does not address
this issue. It installs the package on the host and then generates the same
single-file bind mount.

## Image support

AGS passes selected Fedora packages and verified vendor-archive tools to both
automatic image builds and `ags update-image`. `uv` itself can therefore be
selected through `ags tools`. Installing an arbitrary persistent set of Python
applications with `uv tool install` still requires a separate managed-tool-store
feature.

The image installs `/etc/uv/uv.toml` as a conservative default policy:

- `exclude-newer = "1 week"` delays newly uploaded registry artifacts to provide
  a community-review window.
- `index-strategy = "first-index"` stops at the first index containing a package,
  reducing dependency-confusion exposure.
- `[pip].verify-hashes = true` verifies hashes when requirements provide them
  without requiring every dependency to be hash-pinned.

AGS does not globally require hashes, reject source distributions, ignore
`tool.uv.sources`, or enable uv's preview malware service because those choices
break common development workflows or add an external availability dependency.
Like all uv configuration, project files, environment variables, command-line
arguments, `--config-file`, or `--no-config` can override or bypass the system
default; this is image hardening rather than an enforceable policy boundary.

## Recommendation

- For use now, use `uvx` with one persistent cache mount.
- For AGS itself, add a sandbox-native `python_package` or `uv` tool provider
  and an explicit `update-tools` command.
- For a small mandatory baseline, install tools in the image after image
  extension is properly supported.
- Do not automatically discover and mount host dependencies or virtual
  environments. That approach will remain unreliable across host and container
  upgrades.
