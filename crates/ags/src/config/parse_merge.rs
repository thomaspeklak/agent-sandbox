fn merge_toml_value(base: &mut Value, overlay: Value, path: &[&str]) {
    match (base, overlay) {
        (Value::Table(base_table), Value::Table(overlay_table)) => {
            for (key, overlay_value) in overlay_table {
                if is_additive_array_key(path, &key) {
                    match (base_table.get_mut(&key), overlay_value) {
                        (Some(Value::Array(base_array)), Value::Array(mut overlay_array)) => {
                            base_array.append(&mut overlay_array);
                        }
                        (_, overlay_value) => {
                            base_table.insert(key, overlay_value);
                        }
                    }
                    continue;
                }

                match base_table.get_mut(&key) {
                    Some(base_value) => {
                        let mut child_path = path.to_vec();
                        child_path.push(key.as_str());
                        merge_toml_value(base_value, overlay_value, &child_path);
                    }
                    None => {
                        base_table.insert(key, overlay_value);
                    }
                }
            }
        }
        (base_slot, overlay_value) => {
            *base_slot = overlay_value;
        }
    }
}

fn is_additive_array_key(path: &[&str], key: &str) -> bool {
    path.is_empty() && super::ADDITIVE_ARRAY_KEYS.contains(&key)
}
