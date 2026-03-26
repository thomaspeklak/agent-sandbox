fn parse_string_list(input: &str) -> Vec<String> {
    input
        .split(',')
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .collect()
}

fn parse_key_value_pairs(input: &str) -> Result<Vec<(String, String)>, String> {
    let mut pairs = Vec::new();
    for pair in input
        .split(',')
        .map(str::trim)
        .filter(|pair| !pair.is_empty())
    {
        let Some((key, value)) = pair.split_once('=') else {
            return Err(format!("expected key=value pair, got '{pair}'"));
        };
        let key = key.trim();
        let value = value.trim();
        if key.is_empty() || value.is_empty() {
            return Err(format!("expected non-empty key=value pair, got '{pair}'"));
        }
        pairs.push((key.to_string(), value.to_string()));
    }
    Ok(pairs)
}

fn set_inline_table(entry: &mut toml_edit::Table, key: &str, input: &str) -> Result<(), String> {
    if input.trim().is_empty() {
        entry.remove(key);
        return Ok(());
    }

    let pairs = parse_key_value_pairs(input)?;
    let mut inline = toml_edit::InlineTable::new();
    for (attr_key, attr_value) in pairs {
        inline.insert(
            attr_key.as_str(),
            toml_edit::Value::from(attr_value.as_str()),
        );
    }
    entry[key] = toml_edit::Item::Value(toml_edit::Value::InlineTable(inline));
    Ok(())
}

struct FlattenedEntryParts {
    scalars: Vec<(String, String)>,
    inline_tables: BTreeMap<String, Vec<(String, String)>>,
}

fn parse_flattened_entries(input: &str) -> Result<Vec<FlattenedEntryParts>, String> {
    input
        .split(';')
        .map(str::trim)
        .filter(|entry| !entry.is_empty())
        .map(|entry| {
            let mut parsed = FlattenedEntryParts {
                scalars: Vec::new(),
                inline_tables: BTreeMap::new(),
            };
            for (key, value) in parse_key_value_pairs(entry)? {
                if let Some((prefix, subkey)) = key.split_once('.') {
                    parsed
                        .inline_tables
                        .entry(prefix.to_string())
                        .or_default()
                        .push((subkey.to_string(), value));
                } else {
                    parsed.scalars.push((key, value));
                }
            }
            Ok(parsed)
        })
        .collect()
}

fn flattened_scalar_item(raw: &str) -> toml_edit::Item {
    if let Ok(value) = raw.parse::<bool>() {
        return toml_edit::value(value);
    }
    if let Ok(value) = raw.parse::<i64>() {
        return toml_edit::value(value);
    }
    if let Ok(value) = raw.parse::<f64>() {
        return toml_edit::value(value);
    }
    toml_edit::value(raw)
}

fn set_flattened_nested_entries(
    entry: &mut toml_edit::Table,
    nested_key: &str,
    input: &str,
) -> Result<(), String> {
    if input.trim().is_empty() {
        entry.remove(nested_key);
        return Ok(());
    }

    let mut nested_entries = toml_edit::ArrayOfTables::new();
    for parsed in parse_flattened_entries(input)? {
        let mut nested_entry = toml_edit::Table::new();
        for (key, value) in parsed.scalars {
            nested_entry[key.as_str()] = flattened_scalar_item(&value);
        }
        for (inline_key, pairs) in parsed.inline_tables {
            let mut inline = toml_edit::InlineTable::new();
            for (key, value) in pairs {
                let item = flattened_scalar_item(&value);
                if let Some(value) = item.as_value() {
                    inline.insert(key.as_str(), value.clone());
                }
            }
            nested_entry[inline_key.as_str()] =
                toml_edit::Item::Value(toml_edit::Value::InlineTable(inline));
        }
        nested_entries.push(nested_entry);
    }

    entry[nested_key] = toml_edit::Item::ArrayOfTables(nested_entries);
    Ok(())
}

fn set_nested_mount_entries(entry: &mut toml_edit::Table, input: &str) -> Result<(), String> {
    set_flattened_nested_entries(entry, "directory", input)
}

fn set_nested_secret_entries(entry: &mut toml_edit::Table, input: &str) -> Result<(), String> {
    set_flattened_nested_entries(entry, "secret", input)
}

fn apply_entry_form(
    section_key: &str,
    entry: &mut toml_edit::Table,
    field_values: &[(&'static str, FieldKind, String)],
) -> Result<(), String> {
    for &(key, kind, ref value) in field_values {
        match (key, kind) {
            ("secret_store", _) | ("attributes", _) => set_inline_table(entry, key, value)?,
            ("directories", _) => set_nested_mount_entries(entry, value)?,
            ("secrets", _) => set_nested_secret_entries(entry, value)?,
            (_, FieldKind::Checkbox) => {
                entry[key] = toml_edit::value(value == "true");
            }
            (_, FieldKind::Toggle(_)) => {
                entry[key] = toml_edit::value(value.as_str());
            }
            (_, FieldKind::Text) => {
                if value.trim().is_empty() {
                    entry.remove(key);
                } else {
                    entry[key] = toml_edit::value(value.as_str());
                }
            }
        }
    }

    if section_key == "secret" {
        let env = entry
            .get("env")
            .and_then(|item| item.as_str())
            .unwrap_or_default()
            .to_string();
        let has_from_env = entry
            .get("from_env")
            .and_then(|item| item.as_str())
            .is_some();
        let has_secret_store = entry.get("secret_store").is_some();
        let has_provider = entry
            .get("provider")
            .and_then(|item| item.as_str())
            .is_some();
        if !env.is_empty() && !has_from_env && !has_secret_store && !has_provider {
            entry["from_env"] = toml_edit::value(env);
        }
    }

    Ok(())
}

fn apply_scalar_value(
    doc: &mut toml_edit::DocumentMut,
    section_key: &str,
    field_key: &str,
    kind: ScalarFieldKind,
    new_text: &str,
) -> Result<(), String> {
    ensure_table(doc, section_key);

    match kind {
        ScalarFieldKind::Text => {
            doc[section_key][field_key] = toml_edit::value(new_text);
        }
        ScalarFieldKind::Enum(options) => {
            if !options.contains(&new_text) {
                return Err(format!(
                    "{section_key}.{field_key} must be one of: {}",
                    options.join(", ")
                ));
            }
            doc[section_key][field_key] = toml_edit::value(new_text);
        }
        ScalarFieldKind::Bool => {
            let value = new_text
                .parse::<bool>()
                .map_err(|_| format!("{section_key}.{field_key} must be 'true' or 'false'"))?;
            doc[section_key][field_key] = toml_edit::value(value);
        }
        ScalarFieldKind::Number { min, max } => {
            let value = new_text
                .parse::<u64>()
                .map_err(|_| format!("{section_key}.{field_key} must be a whole number"))?;
            if !(min..=max).contains(&value) {
                return Err(format!(
                    "{section_key}.{field_key} must be between {min} and {max}"
                ));
            }
            let value = i64::try_from(value)
                .map_err(|_| format!("{section_key}.{field_key} is too large to store"))?;
            doc[section_key][field_key] = toml_edit::value(value);
        }
        ScalarFieldKind::StringList => {
            let values = parse_string_list(new_text);
            let array = toml_edit::Array::from_iter(values.iter().map(|value| value.as_str()));
            doc[section_key][field_key] = toml_edit::Item::Value(toml_edit::Value::Array(array));
        }
    }

    Ok(())
}

fn next_toggle_action(
    doc: &toml_edit::DocumentMut,
    section_key: &str,
    field_key: &str,
    schema: &ScalarFieldSchema,
) -> Option<ToggleAction> {
    let item = doc.get(section_key).and_then(|table| table.get(field_key));

    match schema.kind {
        ScalarFieldKind::Bool => {
            let current = item
                .and_then(|item| item.as_bool())
                .unwrap_or(schema.default_input == "true");
            Some(ToggleAction::Bool(!current))
        }
        ScalarFieldKind::Enum(options) => {
            let current = item
                .and_then(|item| item.as_str())
                .unwrap_or(schema.default_input);
            let index = options
                .iter()
                .position(|option| *option == current)
                .unwrap_or(0);
            Some(ToggleAction::Str(options[(index + 1) % options.len()]))
        }
        _ => None,
    }
}

fn global_array_len(doc: &toml_edit::DocumentMut, section_key: &str) -> usize {
    doc.get(section_key)
        .and_then(|item| item.as_array_of_tables())
        .map(|entries| entries.len())
        .unwrap_or(0)
}

fn scalar_field_index_for_target(
    state: &ConfigEditorState,
    target: EditTarget,
    section_key: &str,
    field_key: &str,
) -> Option<usize> {
    let doc = match target {
        EditTarget::Global => &state.global_doc,
        EditTarget::Local => &state.local_doc,
    };

    let content = if let Some(section_idx) = SECTIONS
        .iter()
        .position(|section| section.toml_key == section_key)
    {
        let info = &SECTIONS[section_idx];
        let mut fields = Vec::new();
        let table = doc.get(info.toml_key).and_then(|item| item.as_table_like());
        for schema in scalar_fields(info.toml_key) {
            let present = table
                .and_then(|table| table.get(schema.key))
                .is_some_and(|item| !item.is_none());
            fields.push((schema.key, present));
        }
        if let Some(table) = table {
            for (key, _) in table.iter() {
                if scalar_field(info.toml_key, key).is_none() {
                    fields.push((key, true));
                }
            }
        }
        fields
    } else {
        Vec::new()
    };

    content.iter().position(|(key, _)| *key == field_key)
}

fn edit_target_label(target: EditTarget) -> &'static str {
    match target {
        EditTarget::Global => "global",
        EditTarget::Local => "local",
    }
}

fn expand_home_path(path: &str) -> std::path::PathBuf {
    if let Some(rest) = path.strip_prefix("~/")
        && let Some(home) = dirs::home_dir()
    {
        return home.join(rest);
    }
    std::path::PathBuf::from(path)
}

fn compute_host_status(agent: &AgentDef) -> HostStatus {
    let total = agent.mounts.len();
    if total == 0 {
        return HostStatus::Present;
    }
    let existing = agent
        .mounts
        .iter()
        .filter(|mount| expand_home_path(mount.host).exists())
        .count();
    if existing == 0 {
        HostStatus::Missing
    } else if existing == total {
        HostStatus::Present
    } else {
        HostStatus::Partial(existing, total)
    }
}

/// Format a TOML item as a display string.
fn format_toml_value(item: &toml_edit::Item) -> String {
    match item {
        toml_edit::Item::Value(v) => {
            let s = v.to_string();
            let trimmed = s.trim();
            if trimmed.len() == s.len() {
                s
            } else {
                trimmed.to_owned()
            }
        }
        toml_edit::Item::Table(t) => {
            // Nested table -- show as inline summary
            let keys: Vec<&str> = t.iter().map(|(k, _)| k).collect();
            format!("{{ {} }}", keys.join(", "))
        }
        _ => "(...)".to_string(),
    }
}

/// Map a key code to a tui-input request. `home_end` enables Home/End navigation.
fn key_to_input_request(code: KeyCode, home_end: bool) -> Option<InputRequest> {
    match code {
        KeyCode::Char(c) => Some(InputRequest::InsertChar(c)),
        KeyCode::Backspace => Some(InputRequest::DeletePrevChar),
        KeyCode::Delete => Some(InputRequest::DeleteNextChar),
        KeyCode::Left => Some(InputRequest::GoToPrevChar),
        KeyCode::Right => Some(InputRequest::GoToNextChar),
        KeyCode::Home if home_end => Some(InputRequest::GoToStart),
        KeyCode::End if home_end => Some(InputRequest::GoToEnd),
        _ => None,
    }
}

/// Get a string value from a TOML table, returning a default if missing or non-string.
fn get_str<'a>(table: &'a toml_edit::Table, key: &str, default: &'a str) -> &'a str {
    table.get(key).and_then(|v| v.as_str()).unwrap_or(default)
}

/// Build a one-line summary for an array-of-tables entry based on its section type.
fn summarize_array_entry(toml_key: &str, table: &toml_edit::Table) -> String {
    match toml_key {
        "mount" | "agent_mount" => {
            let host = get_str(table, "host", "?");
            let container = get_str(table, "container", "?");
            let mode = get_str(table, "mode", "ro");
            let kind = get_str(table, "kind", "dir");

            let mut parts = format!("{} -> {}  [{}] [{}]", host, container, mode, kind);

            if table
                .get("optional")
                .and_then(|v| v.as_bool())
                .unwrap_or(false)
            {
                parts.push_str(" [optional]");
            }

            parts
        }
        "tool" => {
            let name = get_str(table, "name", "?");
            let path = get_str(table, "path", "?");
            let container_path = get_str(table, "container_path", "?");
            let mode = get_str(table, "mode", "ro");

            format!("{}  {} -> {}  [{}]", name, path, container_path, mode)
        }
        "secret" => {
            let env = get_str(table, "env", "?");

            if let Some(from_env) = table.get("from_env").and_then(|v| v.as_str()) {
                format!("{}  from_env={}", env, from_env)
            } else if table.get("secret_store").is_some() {
                format!("{}  secret_store", env)
            } else {
                env.to_string()
            }
        }
        _ => {
            // Generic fallback: show all key=value pairs
            let pairs: Vec<String> = table
                .iter()
                .map(|(k, v)| format!("{}={}", k, v.as_str().unwrap_or("...")))
                .collect();
            pairs.join("  ")
        }
    }
}

/// Return (marker_text, style) for a ValueSource, or empty if None (raw view).
fn source_suffix(source: Option<ValueSource>) -> (&'static str, Style) {
    match source {
        None => ("", Style::default()),
        Some(ValueSource::Global) => (" [G]", Style::default().fg(Color::DarkGray)),
        Some(ValueSource::Local) => (" [L]", Style::default().fg(Color::Cyan)),
        Some(ValueSource::LocalOverridesGlobal) => {
            (" [L>G]", Style::default().fg(Color::Yellow).bold())
        }
    }
}
