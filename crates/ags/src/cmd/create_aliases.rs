use std::fmt;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use crate::cli::{AliasMode, CreateAliasesOptions, Shell};

const WRAPPER_MARKER: &str = "# AGS_MANAGED_ALIAS";
const BLOCK_START: &str = "# >>> ags managed aliases >>>";
const BLOCK_END: &str = "# <<< ags managed aliases <<<";

#[derive(Debug)]
pub enum CreateAliasesError {
    HomeDir,
    ShellAutodetect,
    Io { path: PathBuf, source: io::Error },
}

impl fmt::Display for CreateAliasesError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::HomeDir => f.write_str("could not determine home directory"),
            Self::ShellAutodetect => {
                f.write_str("could not autodetect shell; use --shell fish|zsh|bash")
            }
            Self::Io { path, source } => write!(f, "{}: {source}", path.display()),
        }
    }
}

impl std::error::Error for CreateAliasesError {}

#[derive(Clone, Copy)]
struct AliasSpec {
    name: &'static str,
    command: &'static str,
}

const ALIASES: &[AliasSpec] = &[
    // Short names
    AliasSpec {
        name: "asco",
        command: "claude --model opus --strict-mcp-config --dangerously-skip-permissions",
    },
    AliasSpec {
        name: "ascs",
        command: "claude --model sonnet --strict-mcp-config --dangerously-skip-permissions",
    },
    AliasSpec {
        name: "asch",
        command: "claude --model haiku --strict-mcp-config --dangerously-skip-permissions",
    },
    AliasSpec {
        name: "aspi",
        command: "ags --agent pi --",
    },
    AliasSpec {
        name: "asoc",
        command: "ags --agent opencode --",
    },
    AliasSpec {
        name: "asx",
        command: "ags --agent codex --",
    },
    AliasSpec {
        name: "asg",
        command: "ags --agent gemini -- --yolo",
    },
    // Long names
    AliasSpec {
        name: "ags-cc-opus",
        command: "claude --model opus --strict-mcp-config --dangerously-skip-permissions",
    },
    AliasSpec {
        name: "ags-cc-sonnet",
        command: "claude --model sonnet --strict-mcp-config --dangerously-skip-permissions",
    },
    AliasSpec {
        name: "ags-cc-haiku",
        command: "claude --model haiku --strict-mcp-config --dangerously-skip-permissions",
    },
    AliasSpec {
        name: "ags-pi",
        command: "ags --agent pi --",
    },
    AliasSpec {
        name: "ags-oc",
        command: "ags --agent opencode --",
    },
    AliasSpec {
        name: "ags-cx",
        command: "ags --agent codex --",
    },
    AliasSpec {
        name: "ags-gem-yolo",
        command: "ags --agent gemini -- --yolo",
    },
];

#[derive(Default)]
struct ApplySummary {
    created: usize,
    updated: usize,
    skipped: usize,
}

pub fn run(opts: &CreateAliasesOptions) -> Result<(), CreateAliasesError> {
    let home = dirs::home_dir().ok_or(CreateAliasesError::HomeDir)?;

    let mut wrapper_summary = ApplySummary::default();
    let mut aliases_updated = false;

    if matches!(opts.mode, AliasMode::Wrappers | AliasMode::Both) {
        let bin_dir = home.join(".local/bin");
        wrapper_summary = apply_wrappers(&bin_dir, opts.force)?;
    }

    if matches!(opts.mode, AliasMode::Aliases | AliasMode::Both) {
        let shell = opts.shell.unwrap_or(detect_shell()?);
        let rc_path = shell_rc_path(&home, shell);
        aliases_updated = upsert_shell_alias_block(&rc_path, shell)?;
        println!("Updated aliases in: {}", rc_path.display());
    }

    if matches!(opts.mode, AliasMode::Wrappers | AliasMode::Both) {
        println!(
            "Wrappers: {} created, {} updated, {} skipped",
            wrapper_summary.created, wrapper_summary.updated, wrapper_summary.skipped
        );
    }
    if matches!(opts.mode, AliasMode::Aliases | AliasMode::Both) {
        if aliases_updated {
            println!("Alias block written.");
        } else {
            println!("Alias block unchanged.");
        }
    }

    Ok(())
}

fn apply_wrappers(bin_dir: &Path, force: bool) -> Result<ApplySummary, CreateAliasesError> {
    create_dir_all(bin_dir)?;

    let mut summary = ApplySummary::default();
    for spec in ALIASES {
        let target = bin_dir.join(spec.name);

        let exists = target.symlink_metadata().is_ok();
        if exists {
            let meta = symlink_metadata(&target)?;
            let ft = meta.file_type();
            if ft.is_dir() {
                eprintln!(
                    "warning: skipping {}; path is a directory",
                    target.display()
                );
                summary.skipped += 1;
                continue;
            }
            if ft.is_symlink() {
                if !force {
                    eprintln!(
                        "warning: skipping {}; existing symlink (use --force to replace)",
                        target.display()
                    );
                    summary.skipped += 1;
                    continue;
                }
                remove_file(&target)?;
                write_wrapper(&target, spec.command)?;
                summary.updated += 1;
                continue;
            }

            let managed = file_contains_marker(&target, WRAPPER_MARKER);
            if managed || force {
                write_wrapper(&target, spec.command)?;
                summary.updated += 1;
            } else {
                eprintln!(
                    "warning: skipping {}; existing non-managed file (use --force to replace)",
                    target.display()
                );
                summary.skipped += 1;
            }
        } else {
            write_wrapper(&target, spec.command)?;
            summary.created += 1;
        }
    }

    Ok(summary)
}

fn write_wrapper(path: &Path, command: &str) -> Result<(), CreateAliasesError> {
    let content = format!(
        "#!/usr/bin/env bash\n{WRAPPER_MARKER}\nset -euo pipefail\nexec {command} \"$@\"\n"
    );
    write(path, content.as_bytes())?;
    set_executable(path)?;
    println!("Wrote wrapper: {}", path.display());
    Ok(())
}

fn file_contains_marker(path: &Path, marker: &str) -> bool {
    fs::read(path)
        .ok()
        .map(|bytes| String::from_utf8_lossy(&bytes).contains(marker))
        .unwrap_or(false)
}

fn upsert_shell_alias_block(path: &Path, shell: Shell) -> Result<bool, CreateAliasesError> {
    if let Some(parent) = path.parent() {
        create_dir_all(parent)?;
    }

    let block = render_alias_block(shell);
    let current = fs::read_to_string(path).unwrap_or_default();
    let updated = upsert_block(&current, &block);

    if updated == current {
        return Ok(false);
    }

    write(path, updated.as_bytes())?;
    Ok(true)
}

fn render_alias_block(shell: Shell) -> String {
    let mut out = String::new();
    out.push_str(BLOCK_START);
    out.push('\n');
    out.push_str("# Generated by `ags create-aliases`\n");

    for spec in ALIASES {
        let escaped = spec.command.replace('\'', "'\\''");
        match shell {
            Shell::Fish | Shell::Zsh | Shell::Bash => {
                out.push_str(&format!("alias {}='{}'\n", spec.name, escaped));
            }
        }
    }

    out.push_str(BLOCK_END);
    out.push('\n');
    out
}

fn upsert_block(current: &str, block: &str) -> String {
    if let Some(start) = current.find(BLOCK_START)
        && let Some(rel_end) = current[start..].find(BLOCK_END)
    {
        let end = start + rel_end + BLOCK_END.len();
        let mut replace_end = end;
        if current[replace_end..].starts_with("\r\n") {
            replace_end += 2;
        } else if current[replace_end..].starts_with('\n') {
            replace_end += 1;
        }

        let mut out = String::with_capacity(current.len() + block.len());
        out.push_str(&current[..start]);
        out.push_str(block);
        out.push_str(&current[replace_end..]);
        return out;
    }

    if current.trim().is_empty() {
        return block.to_owned();
    }

    let mut out = current.to_owned();
    if !out.ends_with('\n') {
        out.push('\n');
    }
    out.push('\n');
    out.push_str(block);
    out
}

fn detect_shell() -> Result<Shell, CreateAliasesError> {
    let raw = std::env::var("SHELL").map_err(|_| CreateAliasesError::ShellAutodetect)?;
    let Some(name) = Path::new(&raw).file_name().and_then(|v| v.to_str()) else {
        return Err(CreateAliasesError::ShellAutodetect);
    };

    match name {
        "fish" => Ok(Shell::Fish),
        "zsh" => Ok(Shell::Zsh),
        "bash" => Ok(Shell::Bash),
        _ => Err(CreateAliasesError::ShellAutodetect),
    }
}

fn shell_rc_path(home: &Path, shell: Shell) -> PathBuf {
    match shell {
        Shell::Fish => home.join(".config/fish/config.fish"),
        Shell::Zsh => home.join(".zshrc"),
        Shell::Bash => home.join(".bashrc"),
    }
}

fn symlink_metadata(path: &Path) -> Result<fs::Metadata, CreateAliasesError> {
    fs::symlink_metadata(path).map_err(|source| CreateAliasesError::Io {
        path: path.to_owned(),
        source,
    })
}

fn create_dir_all(path: &Path) -> Result<(), CreateAliasesError> {
    fs::create_dir_all(path).map_err(|source| CreateAliasesError::Io {
        path: path.to_owned(),
        source,
    })
}

fn remove_file(path: &Path) -> Result<(), CreateAliasesError> {
    fs::remove_file(path).map_err(|source| CreateAliasesError::Io {
        path: path.to_owned(),
        source,
    })
}

fn write(path: &Path, bytes: &[u8]) -> Result<(), CreateAliasesError> {
    fs::write(path, bytes).map_err(|source| CreateAliasesError::Io {
        path: path.to_owned(),
        source,
    })
}

fn set_executable(path: &Path) -> Result<(), CreateAliasesError> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = fs::metadata(path)
            .map_err(|source| CreateAliasesError::Io {
                path: path.to_owned(),
                source,
            })?
            .permissions();
        perms.set_mode(0o755);
        fs::set_permissions(path, perms).map_err(|source| CreateAliasesError::Io {
            path: path.to_owned(),
            source,
        })?;
    }
    Ok(())
}
