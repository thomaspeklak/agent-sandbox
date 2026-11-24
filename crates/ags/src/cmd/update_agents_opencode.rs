use crate::config::{ArchiveMemberMatch, ToolArchiveFormat, ToolDownloadSource};
use crate::util::shell_quote;

pub(super) const RECOVERY_SCRIPT: &str = r#"OPENCODE_ACTIVE=/opt/opencode-home/.opencode
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
recover_opencode_transaction"#;

const VERSION_HELPER: &str = r#"verify_opencode_version() {
  local binary="$1" phase="$2" expected="$3" actual status
  if actual="$("$binary" --version)"; then
    case "$actual" in
      "$expected"|"v$expected"|"opencode $expected"|"opencode v$expected") return 0 ;;
    esac
    printf 'OpenCode %s version mismatch: expected %s, got %q\n' "$phase" "$expected" "$actual" >&2
    return 1
  else
    status="$?"
    printf 'OpenCode %s version check failed: %s --version exited with status %s (expected %s, stdout %q)\n' "$phase" "$binary" "$status" "$expected" "$actual" >&2
    return "$status"
  fi
}"#;

pub(super) fn build_action(download: Option<&ToolDownloadSource>) -> Result<String, String> {
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
    let action = r#"__VERSION_HELPER__
echo '[ags] updating opencode...' >&2
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
verify_opencode_version "$OPENCODE_STAGE/bin/opencode" staged __VERSION__
rm -rf "$OPENCODE_BACKUP"
if [ -e "$OPENCODE_ACTIVE" ]; then mv "$OPENCODE_ACTIVE" "$OPENCODE_BACKUP"; fi
: > "$OPENCODE_TRANSACTION"
mv "$OPENCODE_STAGE" "$OPENCODE_ACTIVE"
verify_opencode_version "$OPENCODE_ACTIVE/bin/opencode" activated __VERSION__
rm -f "$OPENCODE_TRANSACTION"
rm -rf "$OPENCODE_BACKUP"
rm -f "$OPENCODE_ARCHIVE"
trap - EXIT
remove_pnpm_agent opencode-ai opencode"#
        .replace("__VERSION_HELPER__", VERSION_HELPER)
        .replace("__X86_URL__", &shell_quote(&x86_64.url))
        .replace("__X86_SHA__", &shell_quote(&x86_64.sha256))
        .replace("__ARM_URL__", &shell_quote(&aarch64.url))
        .replace("__ARM_SHA__", &shell_quote(&aarch64.sha256))
        .replace("__MEMBER__", &shell_quote(&download.member))
        .replace("__VERSION__", &shell_quote(&download.version));
    Ok(action)
}

#[cfg(test)]
#[path = "update_agents_opencode_tests.rs"]
mod tests;
