fn require_non_empty<'a>(s: &'a str, ctx: &str) -> Result<&'a str, ConfigError> {
    if s.trim().is_empty() {
        return Err(ConfigError::Validation(format!(
            "{ctx} must be a non-empty string"
        )));
    }
    Ok(s)
}

fn validate_string_list(list: &[String], ctx: &str) -> Result<Vec<String>, ConfigError> {
    for (i, s) in list.iter().enumerate() {
        require_non_empty(s, &format!("{ctx}[{i}]"))?;
    }
    Ok(list.to_vec())
}

fn parse_mode(s: &str, ctx: &str) -> Result<MountMode, ConfigError> {
    match s.to_lowercase().as_str() {
        "ro" => Ok(MountMode::Ro),
        "rw" => Ok(MountMode::Rw),
        _ => Err(ConfigError::Validation(format!(
            "{ctx} must be 'ro' or 'rw'"
        ))),
    }
}

fn parse_kind(s: &str, ctx: &str) -> Result<MountKind, ConfigError> {
    match s.to_lowercase().as_str() {
        "dir" => Ok(MountKind::Dir),
        "file" => Ok(MountKind::File),
        _ => Err(ConfigError::Validation(format!(
            "{ctx} must be 'dir' or 'file'"
        ))),
    }
}

fn parse_when(s: &str, ctx: &str) -> Result<MountWhen, ConfigError> {
    match s.to_lowercase().as_str() {
        "always" => Ok(MountWhen::Always),
        "browser" => Ok(MountWhen::Browser),
        _ => Err(ConfigError::Validation(format!(
            "{ctx} must be 'always' or 'browser'"
        ))),
    }
}

fn expand_path(raw: &str, ctx: &str) -> Result<PathBuf, ConfigError> {
    let after_tilde = expand_tilde(raw)?;
    let after_vars = expand_env_vars(&after_tilde);
    let path = PathBuf::from(&after_vars);
    std::path::absolute(&path)
        .map_err(|e| ConfigError::Validation(format!("{ctx}: failed to resolve path '{raw}': {e}")))
}

fn expand_tilde(raw: &str) -> Result<String, ConfigError> {
    if let Some(rest) = raw.strip_prefix('~') {
        if rest.is_empty() || rest.starts_with('/') {
            let home = dirs::home_dir()
                .ok_or_else(|| ConfigError::Validation("cannot determine home directory".into()))?;
            Ok(format!("{}{rest}", home.display()))
        } else {
            // ~user form not supported, pass through
            Ok(raw.to_owned())
        }
    } else {
        Ok(raw.to_owned())
    }
}

/// Expand `$VAR` and `${VAR}` references. Undefined variables are left as-is
/// (matching Python `os.path.expandvars` behavior).
fn expand_env_vars(input: &str) -> String {
    if !input.contains('$') {
        return input.to_owned();
    }

    let mut result = String::with_capacity(input.len());
    let mut chars = input.chars().peekable();

    while let Some(ch) = chars.next() {
        if ch != '$' {
            result.push(ch);
            continue;
        }

        match chars.peek().copied() {
            Some('{') => {
                chars.next();
                let mut name = String::new();
                let mut closed = false;
                for c in chars.by_ref() {
                    if c == '}' {
                        closed = true;
                        break;
                    }
                    name.push(c);
                }
                if closed {
                    match std::env::var(&name) {
                        Ok(val) => result.push_str(&val),
                        Err(_) => {
                            result.push_str("${");
                            result.push_str(&name);
                            result.push('}');
                        }
                    }
                } else {
                    result.push_str("${");
                    result.push_str(&name);
                }
            }
            Some(c) if c.is_ascii_alphabetic() || c == '_' => {
                let mut name = String::new();
                while let Some(&c) = chars.peek() {
                    if c.is_ascii_alphanumeric() || c == '_' {
                        name.push(c);
                        chars.next();
                    } else {
                        break;
                    }
                }
                match std::env::var(&name) {
                    Ok(val) => result.push_str(&val),
                    Err(_) => {
                        result.push('$');
                        result.push_str(&name);
                    }
                }
            }
            _ => {
                result.push('$');
            }
        }
    }

    result
}
