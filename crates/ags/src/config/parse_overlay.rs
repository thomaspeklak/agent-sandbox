fn reject_overlay_command_secrets(overlay: &Value, overlay_path: &Path) -> Result<(), ConfigError> {
    let Some(root) = overlay.as_table() else {
        return Ok(());
    };

    if let Some(secrets) = root.get("secret").and_then(Value::as_array) {
        for (index, secret) in secrets.iter().enumerate() {
            if secret
                .as_table()
                .is_some_and(|table| table.contains_key("command"))
            {
                return Err(ConfigError::Validation(format!(
                    "repo-local config {} may not define [[secret]] #{index}.command; command secret sources are allowed only in the user/global config",
                    overlay_path.display()
                )));
            }
        }
    }

    if let Some(tools) = root.get("tool").and_then(Value::as_array) {
        for (tool_index, tool) in tools.iter().enumerate() {
            let Some(secrets) = tool
                .as_table()
                .and_then(|table| table.get("secret"))
                .and_then(Value::as_array)
            else {
                continue;
            };
            for (secret_index, secret) in secrets.iter().enumerate() {
                if secret
                    .as_table()
                    .is_some_and(|table| table.contains_key("command"))
                {
                    return Err(ConfigError::Validation(format!(
                        "repo-local config {} may not define [[tool]] #{tool_index}.secret[{secret_index}].command; command secret sources are allowed only in the user/global config",
                        overlay_path.display()
                    )));
                }
            }
        }
    }

    Ok(())
}
