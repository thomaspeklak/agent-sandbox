use crate::cli::Agent;
use crate::config::{
    ArchiveMemberMatch, DEFAULT_PI_SPEC, LEGACY_PI_SPECS, ToolArchiveFormat, ToolDownloadSource,
};
use crate::util::shell_quote;

pub(super) fn resolve_pi_spec(spec: &str) -> &str {
    if LEGACY_PI_SPECS.contains(&spec) {
        DEFAULT_PI_SPEC
    } else {
        spec
    }
}

fn legacy_pi_cleanup_script() -> String {
    LEGACY_PI_SPECS
        .iter()
        .map(|spec| format!("remove_legacy_pnpm_agent {} pi\n", shell_quote(spec)))
        .collect()
}

const PNPM_RECONCILE_HELPERS: &str = r#"install_pnpm_agent() {
  local name="$1"; shift
  echo "[ags] updating $name..." >&2
  "$PNPM_BIN" add -g "$@" || return
  command -v "$name" >/dev/null 2>&1 || return
}
pnpm_dependency_path() {
  local package="$1" dependency_list
  dependency_list="$("$PNPM_BIN" list -g --depth=0 --json)" || return
  printf '%s\n' "$dependency_list" | node -e '
const fs = require("fs");
const packageKey = process.argv[1];
const parsed = JSON.parse(fs.readFileSync(0, "utf8"));
const roots = Array.isArray(parsed) ? parsed : [parsed];
const matches = roots.flatMap((root) => Object.entries(root.dependencies || {}))
  .filter(([key]) => key === packageKey)
  .map(([, dependency]) => dependency.path)
  .filter((path) => typeof path === "string");
if (matches.length === 0) process.exit(3);
if (matches.length !== 1) throw new Error(`expected one global dependency for ${packageKey}`);
process.stdout.write(matches[0]);
' "$package"
}
pnpm_dependency_keys_for_bin() {
  local name="$1" dependency_list
  dependency_list="$("$PNPM_BIN" list -g --depth=0 --json)" || return
  printf '%s\n' "$dependency_list" | node -e '
const fs = require("fs");
const path = require("path");
const command = process.argv[1];
const parsed = JSON.parse(fs.readFileSync(0, "utf8"));
const roots = Array.isArray(parsed) ? parsed : [parsed];
const keys = new Set();
for (const root of roots) {
  for (const [key, dependency] of Object.entries(root.dependencies || {})) {
    if (typeof dependency.path !== "string") {
      throw new Error(`global dependency ${key} has no package path`);
    }
    const manifest = JSON.parse(fs.readFileSync(path.join(dependency.path, "package.json"), "utf8"));
    const defaultCommand = String(manifest.name || "").replace(/^@[^/]+\//, "");
    const exposesCommand = typeof manifest.bin === "string"
      ? defaultCommand === command
      : manifest.bin && Object.prototype.hasOwnProperty.call(manifest.bin, command);
    if (exposesCommand) keys.add(key);
  }
}
process.stdout.write([...keys].sort().join("\n"));
' "$name"
}
remove_pnpm_dependency() {
  local package="$1" package_path status
  if package_path="$(pnpm_dependency_path "$package")"; then
    echo "[ags] removing $package..." >&2
    "$PNPM_BIN" remove -g "$package" >/dev/null || return
  else
    status="$?"
    [ "$status" -eq 3 ] && return 0
    return "$status"
  fi
  if pnpm_dependency_path "$package" >/dev/null; then
    echo "failed to remove pnpm dependency $package" >&2
    return 1
  else
    status="$?"
    [ "$status" -eq 3 ] || return "$status"
  fi
}
remove_pnpm_agent() {
  local package="$1" name="$2"
  remove_pnpm_dependency "$package" || return
  rm -f "/usr/local/pnpm/$name" "/usr/local/pnpm/bin/$name" || return
  if [ -e "/usr/local/pnpm/$name" ] || [ -L "/usr/local/pnpm/$name" ] || [ -e "/usr/local/pnpm/bin/$name" ] || [ -L "/usr/local/pnpm/bin/$name" ]; then
    echo "failed to remove $package runtime" >&2
    return 1
  fi
}
backup_unowned_launcher() {
  local launcher="$1" package_path="$2" backup launcher_target
  backup="${launcher}.ags-preserved"
  rm -f "$backup" || return
  if [ -e "$launcher" ] || [ -L "$launcher" ]; then
    launcher_target="$(readlink -f "$launcher" 2>/dev/null || true)"
    case "$launcher_target" in
      "$package_path"|"$package_path"/*) ;;
      *) cp -a "$launcher" "$backup" || return ;;
    esac
  fi
}
restore_preserved_launcher() {
  local launcher="$1" backup
  backup="${launcher}.ags-preserved"
  if [ -e "$backup" ] || [ -L "$backup" ]; then
    rm -f "$launcher" || return
    mv "$backup" "$launcher" || return
  fi
}
remove_owned_legacy_pnpm_agent() {
  local package="$1" name="$2" package_path status root_launcher bin_launcher
  if package_path="$(pnpm_dependency_path "$package")"; then
    :
  else
    status="$?"
    [ "$status" -eq 3 ] && return 0
    return "$status"
  fi
  root_launcher="/usr/local/pnpm/$name"
  bin_launcher="/usr/local/pnpm/bin/$name"
  backup_unowned_launcher "$root_launcher" "$package_path" || return
  backup_unowned_launcher "$bin_launcher" "$package_path" || {
    restore_preserved_launcher "$root_launcher"
    return 1
  }
  if ! "$PNPM_BIN" remove -g "$package" >/dev/null; then
    restore_preserved_launcher "$root_launcher"
    restore_preserved_launcher "$bin_launcher"
    return 1
  fi
  restore_preserved_launcher "$root_launcher" || return
  restore_preserved_launcher "$bin_launcher" || return
  if pnpm_dependency_path "$package" >/dev/null; then
    return 1
  else
    status="$?"
    [ "$status" -eq 3 ] || return "$status"
  fi
}
remove_legacy_pnpm_agent() {
  remove_owned_legacy_pnpm_agent "$@" || echo "warning: could not fully clean obsolete package $1" >&2
}
remove_pnpm_agents_for_bin() {
  local name="$1" packages package remaining
  packages="$(pnpm_dependency_keys_for_bin "$name")" || return
  while IFS= read -r package; do
    [ -z "$package" ] && continue
    remove_pnpm_dependency "$package" || return
  done <<EOF
$packages
EOF
  remaining="$(pnpm_dependency_keys_for_bin "$name")" || return
  if [ -n "$remaining" ]; then
    echo "failed to remove pnpm dependencies providing $name" >&2
    return 1
  fi
  rm -f "/usr/local/pnpm/$name" "/usr/local/pnpm/bin/$name" || return
  if [ -e "/usr/local/pnpm/$name" ] || [ -L "/usr/local/pnpm/$name" ] || [ -e "/usr/local/pnpm/bin/$name" ] || [ -L "/usr/local/pnpm/bin/$name" ]; then
    echo "failed to remove $name runtime" >&2
    return 1
  fi
}"#;

fn opencode_action(download: Option<&ToolDownloadSource>) -> Result<String, String> {
    let Some(download) = download else {
        return Ok(r#"remove_pnpm_agent opencode-ai opencode
echo '[ags] removing opencode data...' >&2
rm -rf /opt/opencode-home/.opencode /opt/opencode-home/.opencode.stage /opt/opencode-home/.opencode.previous
rm -f /opt/opencode-home/.opencode.transaction"#
            .to_owned());
    };
    if download.archive != ToolArchiveFormat::TarGz
        || download.member_match != ArchiveMemberMatch::Exact
        || download.install_as != "opencode"
    {
        return Err(
            "OpenCode release source must install an exact member named 'opencode' from tar.gz"
                .to_owned(),
        );
    }
    let x86_64 = download
        .artifacts
        .get("x86_64")
        .ok_or_else(|| "OpenCode release is missing x86_64 artifact".to_owned())?;
    let aarch64 = download
        .artifacts
        .get("aarch64")
        .ok_or_else(|| "OpenCode release is missing aarch64 artifact".to_owned())?;
    let action = r#"echo '[ags] updating opencode...' >&2
OPENCODE_ACTIVE=/opt/opencode-home/.opencode
OPENCODE_STAGE=/opt/opencode-home/.opencode.stage
OPENCODE_BACKUP=/opt/opencode-home/.opencode.previous
OPENCODE_TRANSACTION=/opt/opencode-home/.opencode.transaction
OPENCODE_ARCHIVE=/tmp/ags-opencode.tar.gz
recover_opencode_transaction() {
  if [ -e "$OPENCODE_TRANSACTION" ]; then
    rm -rf "$OPENCODE_ACTIVE"
    if [ -e "$OPENCODE_BACKUP" ]; then mv "$OPENCODE_BACKUP" "$OPENCODE_ACTIVE"; fi
    rm -f "$OPENCODE_TRANSACTION"
  elif [ ! -e "$OPENCODE_ACTIVE" ] && [ -e "$OPENCODE_BACKUP" ]; then
    mv "$OPENCODE_BACKUP" "$OPENCODE_ACTIVE"
  elif [ -e "$OPENCODE_ACTIVE" ] && [ -e "$OPENCODE_BACKUP" ]; then
    rm -rf "$OPENCODE_BACKUP"
  fi
  rm -rf "$OPENCODE_STAGE"
  rm -f "$OPENCODE_ARCHIVE"
}
recover_opencode_transaction
trap recover_opencode_transaction EXIT
case "$(uname -m)" in
  x86_64|amd64) OPENCODE_URL=__X86_URL__; OPENCODE_SHA256=__X86_SHA__ ;;
  aarch64|arm64) OPENCODE_URL=__ARM_URL__; OPENCODE_SHA256=__ARM_SHA__ ;;
  *) echo 'unsupported architecture for OpenCode' >&2; exit 1 ;;
esac
curl --proto '=https' --proto-redir '=https' --tlsv1.2 -fsSL --connect-timeout 10 --max-time 300 --retry 2 --retry-delay 1 "$OPENCODE_URL" -o "$OPENCODE_ARCHIVE"
printf '%s  %s\n' "$OPENCODE_SHA256" "$OPENCODE_ARCHIVE" | sha256sum -c -
mkdir -p "$OPENCODE_STAGE/bin"
tar -xOzf "$OPENCODE_ARCHIVE" -- __MEMBER__ > "$OPENCODE_STAGE/bin/opencode"
chmod 0755 "$OPENCODE_STAGE/bin/opencode"
OPENCODE_ACTUAL_VERSION="$("$OPENCODE_STAGE/bin/opencode" --version)"
[ "${OPENCODE_ACTUAL_VERSION#v}" = __VERSION__ ]
rm -rf "$OPENCODE_BACKUP"
if [ -e "$OPENCODE_ACTIVE" ]; then mv "$OPENCODE_ACTIVE" "$OPENCODE_BACKUP"; fi
: > "$OPENCODE_TRANSACTION"
mv "$OPENCODE_STAGE" "$OPENCODE_ACTIVE"
OPENCODE_ACTUAL_VERSION="$(/opt/opencode-home/.opencode/bin/opencode --version)"
[ "${OPENCODE_ACTUAL_VERSION#v}" = __VERSION__ ]
rm -f "$OPENCODE_TRANSACTION"
rm -rf "$OPENCODE_BACKUP"
rm -f "$OPENCODE_ARCHIVE"
trap - EXIT
remove_pnpm_agent opencode-ai opencode"#
        .replace("__X86_URL__", &shell_quote(&x86_64.url))
        .replace("__X86_SHA__", &shell_quote(&x86_64.sha256))
        .replace("__ARM_URL__", &shell_quote(&aarch64.url))
        .replace("__ARM_SHA__", &shell_quote(&aarch64.sha256))
        .replace("__MEMBER__", &shell_quote(&download.member))
        .replace("__VERSION__", &shell_quote(&download.version));
    Ok(action)
}

pub(super) fn build_install_script(
    pi_spec: &str,
    release_age: u32,
    enabled_agents: &[Agent],
    opencode_download: Option<&ToolDownloadSource>,
) -> Result<String, String> {
    let pi_spec = shell_quote(pi_spec);
    let legacy_pi_cleanup = legacy_pi_cleanup_script();
    let pi_action = if enabled_agents.contains(&Agent::Pi) {
        format!("install_pnpm_agent pi {pi_spec}\n{legacy_pi_cleanup}command -v pi >/dev/null 2>&1")
    } else {
        "remove_pnpm_agents_for_bin pi".to_owned()
    };
    let codex_action = if enabled_agents.contains(&Agent::Codex) {
        r#"echo '[ags] updating codex...' >&2
curl -fsSL https://chatgpt.com/codex/install.sh -o /tmp/codex-install.sh
CODEX_HOME=/opt/codex-home CODEX_INSTALL_DIR=/usr/local/pnpm CODEX_NON_INTERACTIVE=true sh /tmp/codex-install.sh
[ -x /usr/local/pnpm/codex ]
remove_legacy_pnpm_agent @openai/codex codex
[ -x /usr/local/pnpm/codex ]"#
    } else {
        r#"remove_pnpm_agent @openai/codex codex
echo '[ags] removing codex data...' >&2
rm -rf /opt/codex-home/* /opt/codex-home/.[!.]* /opt/codex-home/..?*"#
    };
    let gemini_action = if enabled_agents.contains(&Agent::Gemini) {
        "install_pnpm_agent gemini @google/gemini-cli"
    } else {
        "remove_pnpm_agent @google/gemini-cli gemini"
    };
    let opencode_action = if enabled_agents.contains(&Agent::Opencode) {
        let download = opencode_download
            .ok_or_else(|| "enabled OpenCode agent has no resolved release".to_owned())?;
        opencode_action(Some(download))?
    } else {
        opencode_action(None)?
    };
    let claude_action = if enabled_agents.contains(&Agent::Claude) {
        r#"CLAUDE_HOME=/opt/claude-home
CLAUDE_BIN="$CLAUDE_HOME/.local/bin/claude"
if [ -x "$CLAUDE_BIN" ]; then
  HOME="$CLAUDE_HOME" PATH="$CLAUDE_HOME/.local/bin:$PATH" "$CLAUDE_BIN" update || {
    echo 'claude update failed; reinstalling via install.sh' >&2
    export HOME="$CLAUDE_HOME" PATH="$CLAUDE_HOME/.local/bin:$PATH"
    curl -fsSL https://claude.ai/install.sh | bash
  }
else
  export HOME="$CLAUDE_HOME" PATH="$CLAUDE_HOME/.local/bin:$PATH"
  curl -fsSL https://claude.ai/install.sh | bash
fi
[ -x "$CLAUDE_BIN" ]
rm -f /usr/local/pnpm/claude
printf '%s\n' '#!/usr/bin/env bash' 'export PATH=/opt/claude-home/.local/bin:$PATH' 'exec /opt/claude-home/.local/bin/claude "$@"' > /usr/local/pnpm/claude
chmod +x /usr/local/pnpm/claude"#
    } else {
        r#"echo '[ags] removing claude...' >&2
rm -f /usr/local/pnpm/claude
rm -rf /opt/claude-home/* /opt/claude-home/.[!.]* /opt/claude-home/..?*"#
    };

    let helpers = PNPM_RECONCILE_HELPERS;
    Ok(format!(
        r#"set -e
mkdir -p "$HOME/.config/pnpm" /usr/local/pnpm /opt/codex-home /opt/claude-home
printf 'minimum-release-age=%s\nignore-scripts=true\nstore-dir=/usr/local/pnpm/.store\nglobal-bin-dir=/usr/local/pnpm\n' '{release_age}' > "$HOME/.config/pnpm/rc"
export PNPM_HOME=/usr/local/pnpm NPM_CONFIG_STORE_DIR=/usr/local/pnpm/.store NPM_CONFIG_GLOBAL_BIN_DIR=/usr/local/pnpm PATH=/usr/local/bin:/usr/bin:/bin:/usr/local/pnpm:/usr/local/pnpm/bin:$PATH
PNPM_BIN=/usr/local/bin/pnpm
if ! [ -x "$PNPM_BIN" ] || ! "$PNPM_BIN" --version >/dev/null; then
  echo "sandbox pnpm is unavailable; run 'ags update-image'" >&2
  exit 1
fi
rm -f /usr/local/pnpm/pnpm /usr/local/pnpm/pn /usr/local/pnpm/pnpx /usr/local/pnpm/pnx /usr/local/pnpm/bin/pnpm /usr/local/pnpm/bin/pn /usr/local/pnpm/bin/pnpx /usr/local/pnpm/bin/pnx
rm -f /home/dev/.npm-global/bin/pi /home/dev/.npm-global/bin/codex /home/dev/.npm-global/bin/gemini /home/dev/.npm-global/bin/opencode
rm -rf /home/dev/.npm-global/lib/node_modules/@mariozechner/pi-coding-agent /home/dev/.npm-global/lib/node_modules/@earendil-works/pi-coding-agent /home/dev/.npm-global/lib/node_modules/@openai/codex /home/dev/.npm-global/lib/node_modules/@google/gemini-cli /home/dev/.npm-global/lib/node_modules/opencode-ai
{helpers}
{pi_action}
{codex_action}
{gemini_action}
{opencode_action}
{claude_action}
"$PNPM_BIN" store prune
"#,
    ))
}
