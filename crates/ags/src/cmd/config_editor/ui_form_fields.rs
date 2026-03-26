// ---------------------------------------------------------------------------
// Form field builders
// ---------------------------------------------------------------------------

fn form_field(
    label: &'static str,
    key: &'static str,
    kind: FieldKind,
    required: bool,
    value: String,
) -> FormField {
    FormField {
        label,
        key,
        kind,
        required,
        input: Input::new(value),
    }
}

fn bool_form_value(table: Option<&toml_edit::Table>, key: &str, default: bool) -> String {
    table
        .and_then(|table| table.get(key))
        .and_then(|item| item.as_bool())
        .unwrap_or(default)
        .to_string()
}

fn string_form_value(table: Option<&toml_edit::Table>, key: &str, default: &str) -> String {
    table
        .and_then(|table| table.get(key))
        .and_then(|item| item.as_str())
        .unwrap_or(default)
        .to_string()
}

fn kv_table_form_value(table: Option<&toml_edit::Table>, key: &str) -> String {
    table
        .and_then(|table| table.get(key))
        .and_then(|item| item.as_inline_table())
        .map(|inline| {
            inline
                .iter()
                .map(|(k, v)| format!("{}={}", k, v.as_str().unwrap_or(&v.to_string())))
                .collect::<Vec<_>>()
                .join(", ")
        })
        .unwrap_or_default()
}

fn serialize_nested_mount_entries(table: Option<&toml_edit::Table>) -> String {
    table
        .and_then(|table| table.get("directory"))
        .and_then(|item| item.as_array_of_tables())
        .map(|entries| {
            entries
                .iter()
                .map(|entry| {
                    serialize_flattened_table_entry(
                        entry,
                        &[
                            "host",
                            "container",
                            "mode",
                            "kind",
                            "when",
                            "create",
                            "optional",
                            "source",
                        ],
                        &[],
                    )
                })
                .collect::<Vec<_>>()
                .join("; ")
        })
        .unwrap_or_default()
}

fn serialize_nested_secret_entries(table: Option<&toml_edit::Table>) -> String {
    table
        .and_then(|table| table.get("secret"))
        .and_then(|item| item.as_array_of_tables())
        .map(|entries| {
            entries
                .iter()
                .map(|entry| {
                    serialize_flattened_table_entry(
                        entry,
                        &["env", "from_env", "provider", "var"],
                        &["secret_store", "attributes"],
                    )
                })
                .collect::<Vec<_>>()
                .join("; ")
        })
        .unwrap_or_default()
}

fn serialize_flattened_table_entry(
    table: &toml_edit::Table,
    preferred_scalar_keys: &[&str],
    preferred_inline_keys: &[&str],
) -> String {
    let mut parts = Vec::new();

    for key in preferred_scalar_keys {
        if let Some(item) = table.get(key)
            && let Some(part) = serialize_flattened_scalar_part(key, item)
        {
            parts.push(part);
        }
    }

    for (key, item) in table.iter() {
        if preferred_scalar_keys.contains(&key) || preferred_inline_keys.contains(&key) {
            continue;
        }
        if let Some(part) = serialize_flattened_scalar_part(key, item) {
            parts.push(part);
        }
    }

    for key in preferred_inline_keys {
        if let Some(inline) = table.get(key).and_then(|item| item.as_inline_table()) {
            parts.extend(serialize_inline_table_parts(key, inline));
        }
    }

    for (key, item) in table.iter() {
        if preferred_inline_keys.contains(&key) {
            continue;
        }
        if let Some(inline) = item.as_inline_table() {
            parts.extend(serialize_inline_table_parts(key, inline));
        }
    }

    parts.join(", ")
}

fn serialize_flattened_scalar_part(key: &str, item: &toml_edit::Item) -> Option<String> {
    let value = item.as_value()?;
    if matches!(value, toml_edit::Value::InlineTable(_)) {
        return None;
    }
    Some(format!("{key}={}", serialize_flattened_value(value)))
}

fn serialize_inline_table_parts(prefix: &str, inline: &toml_edit::InlineTable) -> Vec<String> {
    inline
        .iter()
        .map(|(key, value)| format!("{}.{}={}", prefix, key, serialize_flattened_value(value)))
        .collect()
}

fn serialize_flattened_value(value: &toml_edit::Value) -> String {
    value
        .as_str()
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| value.to_string())
}

/// Shared fields for `[[mount]]` and `[[agent_mount]]`: host, container, kind.
fn base_mount_fields(table: Option<&toml_edit::Table>) -> Vec<FormField> {
    vec![
        form_field(
            "host",
            "host",
            FieldKind::Text,
            true,
            string_form_value(table, "host", ""),
        ),
        form_field(
            "container",
            "container",
            FieldKind::Text,
            true,
            string_form_value(table, "container", ""),
        ),
        form_field(
            "kind",
            "kind",
            FieldKind::Toggle(&["dir", "file"]),
            false,
            string_form_value(table, "kind", "dir"),
        ),
    ]
}

fn build_entry_form_fields(section_key: &str, table: Option<&toml_edit::Table>) -> Vec<FormField> {
    match section_key {
        "mount" => {
            let mut fields = base_mount_fields(table);
            fields.insert(
                2,
                form_field(
                    "mode",
                    "mode",
                    FieldKind::Toggle(&["ro", "rw"]),
                    false,
                    string_form_value(table, "mode", "ro"),
                ),
            );
            fields.insert(
                4,
                form_field(
                    "when",
                    "when",
                    FieldKind::Toggle(&["always", "browser"]),
                    false,
                    string_form_value(table, "when", "always"),
                ),
            );
            fields.push(form_field(
                "source",
                "source",
                FieldKind::Text,
                false,
                string_form_value(table, "source", "config"),
            ));
            fields.push(form_field(
                "create",
                "create",
                FieldKind::Checkbox,
                false,
                bool_form_value(table, "create", false),
            ));
            fields.push(form_field(
                "optional",
                "optional",
                FieldKind::Checkbox,
                false,
                bool_form_value(table, "optional", false),
            ));
            fields
        }
        "agent_mount" => base_mount_fields(table),
        "secret" => vec![
            form_field(
                "env",
                "env",
                FieldKind::Text,
                true,
                string_form_value(table, "env", ""),
            ),
            form_field(
                "from_env",
                "from_env",
                FieldKind::Text,
                false,
                string_form_value(table, "from_env", ""),
            ),
            form_field(
                "secret_store",
                "secret_store",
                FieldKind::Text,
                false,
                kv_table_form_value(table, "secret_store"),
            ),
            form_field(
                "provider",
                "provider",
                FieldKind::Text,
                false,
                string_form_value(table, "provider", ""),
            ),
            form_field(
                "var",
                "var",
                FieldKind::Text,
                false,
                string_form_value(table, "var", ""),
            ),
            form_field(
                "attributes",
                "attributes",
                FieldKind::Text,
                false,
                kv_table_form_value(table, "attributes"),
            ),
        ],
        "tool" => vec![
            form_field(
                "name",
                "name",
                FieldKind::Text,
                true,
                string_form_value(table, "name", ""),
            ),
            form_field(
                "path",
                "path",
                FieldKind::Text,
                true,
                string_form_value(table, "path", ""),
            ),
            form_field(
                "container_path",
                "container_path",
                FieldKind::Text,
                true,
                string_form_value(table, "container_path", ""),
            ),
            form_field(
                "mode",
                "mode",
                FieldKind::Toggle(&["ro", "rw"]),
                false,
                string_form_value(table, "mode", "ro"),
            ),
            form_field(
                "when",
                "when",
                FieldKind::Toggle(&["always", "browser"]),
                false,
                string_form_value(table, "when", "always"),
            ),
            form_field(
                "optional",
                "optional",
                FieldKind::Checkbox,
                false,
                bool_form_value(table, "optional", false),
            ),
            form_field(
                "directories",
                "directories",
                FieldKind::Text,
                false,
                serialize_nested_mount_entries(table),
            ),
            form_field(
                "secrets",
                "secrets",
                FieldKind::Text,
                false,
                serialize_nested_secret_entries(table),
            ),
        ],
        _ => vec![],
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn suggestion_for_field(
    field_key: &str,
    input: &str,
    cache: &SuggestionCache<'_>,
) -> Option<String> {
    let query = input.trim();
    if query.is_empty() {
        return None;
    }

    match field_key {
        "command" | "binary" => suggest_binaries_from(query, cache.binaries)
            .into_iter()
            .next()
            .map(|s| s.value),
        "host" | "path" | "containerfile" | "cache_dir" | "gitconfig_path" | "auth_key"
        | "sign_key" | "profile_dir" | "renderer_bin" => suggest_paths_from(query, cache.home_dirs)
            .into_iter()
            .next()
            .map(|s| s.value),
        _ => None,
    }
}

fn ensure_table(doc: &mut toml_edit::DocumentMut, section_key: &str) {
    if doc
        .get(section_key)
        .and_then(|item| item.as_table_like())
        .is_none()
    {
        doc[section_key] = toml_edit::Item::Table(toml_edit::Table::new());
    }
}

fn missing_scalar_value(schema: &ScalarFieldSchema) -> String {
    if schema.default_input.is_empty() {
        "<unset>".to_string()
    } else {
        match schema.kind {
            ScalarFieldKind::StringList => format!("<default: [{}]>", schema.default_input),
            _ => format!("<default: {}>", schema.default_input),
        }
    }
}

fn item_to_editor_text(item: &toml_edit::Item, schema: &ScalarFieldSchema) -> String {
    match schema.kind {
        ScalarFieldKind::Text | ScalarFieldKind::Enum(_) => item
            .as_str()
            .map(ToOwned::to_owned)
            .unwrap_or_else(|| format_toml_value(item)),
        ScalarFieldKind::Bool => item
            .as_bool()
            .map(|value| value.to_string())
            .unwrap_or_else(|| schema.default_input.to_string()),
        ScalarFieldKind::Number { .. } => item
            .as_integer()
            .map(|value| value.to_string())
            .or_else(|| item.as_float().map(|value| value.to_string()))
            .unwrap_or_else(|| schema.default_input.to_string()),
        ScalarFieldKind::StringList => item
            .as_array()
            .map(|array| {
                array
                    .iter()
                    .filter_map(|value| value.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            })
            .unwrap_or_else(|| schema.default_input.to_string()),
    }
}
