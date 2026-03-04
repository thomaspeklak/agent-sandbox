use std::env;
use std::path::PathBuf;

/// Expand a config path: resolve `~` to home dir, `$VAR` to env vars,
/// and canonicalize relative paths against the current directory.
///
/// Mirrors legacy shell resolver path-expansion behavior.
pub fn expand_path(raw: &str) -> Result<PathBuf, PathExpandError> {
    let after_tilde = expand_tilde(raw)?;
    let after_vars = expand_env_vars(&after_tilde)?;
    let path = PathBuf::from(&after_vars);

    if path.is_absolute() {
        Ok(path)
    } else {
        let cwd = env::current_dir().map_err(|e| PathExpandError::CurrentDir(e.to_string()))?;
        Ok(cwd.join(path))
    }
}

/// Expand leading `~` or `~/` to the user's home directory.
fn expand_tilde(path: &str) -> Result<String, PathExpandError> {
    if path == "~" || path.starts_with("~/") {
        let home = home_dir()?;
        Ok(path.replacen('~', &home, 1))
    } else {
        Ok(path.to_owned())
    }
}

/// Expand `$VAR` and `${VAR}` references in a path string.
fn expand_env_vars(input: &str) -> Result<String, PathExpandError> {
    let mut result = String::with_capacity(input.len());
    let mut chars = input.chars().peekable();

    while let Some(ch) = chars.next() {
        if ch != '$' {
            result.push(ch);
            continue;
        }

        let var_name = if chars.peek() == Some(&'{') {
            chars.next(); // consume '{'
            let name: String = chars.by_ref().take_while(|&c| c != '}').collect();
            name
        } else {
            let mut name = String::new();
            while let Some(&c) = chars.peek() {
                if c.is_ascii_alphanumeric() || c == '_' {
                    name.push(c);
                    chars.next();
                } else {
                    break;
                }
            }
            name
        };

        if var_name.is_empty() {
            result.push('$');
            continue;
        }

        let value = env::var(&var_name).map_err(|_| PathExpandError::EnvVar(var_name.clone()))?;
        result.push_str(&value);
    }

    Ok(result)
}

fn home_dir() -> Result<String, PathExpandError> {
    env::var("HOME").map_err(|_| PathExpandError::NoHome)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PathExpandError {
    NoHome,
    EnvVar(String),
    CurrentDir(String),
}

impl std::fmt::Display for PathExpandError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NoHome => f.write_str("HOME environment variable not set"),
            Self::EnvVar(var) => write!(f, "environment variable ${var} not set"),
            Self::CurrentDir(err) => write!(f, "failed to get current directory: {err}"),
        }
    }
}

impl std::error::Error for PathExpandError {}
