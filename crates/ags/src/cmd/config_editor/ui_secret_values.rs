fn split_top_level(input: &str, delimiter: char) -> impl Iterator<Item = &str> {
    let mut parts = Vec::new();
    let mut start = 0;
    let mut quote = None;
    let mut escaped = false;
    let mut square_depth = 0_u32;
    let mut curly_depth = 0_u32;

    for (index, ch) in input.char_indices() {
        if let Some(active_quote) = quote {
            if active_quote == '"' && escaped {
                escaped = false;
            } else if active_quote == '"' && ch == '\\' {
                escaped = true;
            } else if ch == active_quote {
                quote = None;
            }
            continue;
        }

        match ch {
            '"' | '\'' => quote = Some(ch),
            '[' => square_depth += 1,
            ']' => square_depth = square_depth.saturating_sub(1),
            '{' => curly_depth += 1,
            '}' => curly_depth = curly_depth.saturating_sub(1),
            _ if ch == delimiter && square_depth == 0 && curly_depth == 0 => {
                parts.push(&input[start..index]);
                start = index + ch.len_utf8();
            }
            _ => {}
        }
    }
    parts.push(&input[start..]);
    parts.into_iter()
}

fn set_string_array(entry: &mut toml_edit::Table, key: &str, input: &str) -> Result<(), String> {
    if input.trim().is_empty() {
        entry.remove(key);
        return Ok(());
    }

    entry[key] = string_array_item(key, input)?;
    Ok(())
}

fn string_array_item(key: &str, input: &str) -> Result<toml_edit::Item, String> {
    let document = format!("value = {input}")
        .parse::<toml_edit::DocumentMut>()
        .map_err(|error| format!("{key} must be a TOML string array: {error}"))?;
    let array = document["value"]
        .as_array()
        .ok_or_else(|| format!("{key} must be a TOML string array"))?;
    if array.iter().any(|value| value.as_str().is_none()) {
        return Err(format!("{key} must contain only strings"));
    }
    Ok(toml_edit::Item::Value(toml_edit::Value::Array(
        array.clone(),
    )))
}
