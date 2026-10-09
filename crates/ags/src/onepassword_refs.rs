//! Central prepare-hook field-reference resolution via op's supported inject batch.
//! References are metadata until the final launch handoff; values are never logged.
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::process::Command;
use std::sync::atomic::AtomicBool;
use std::time::{Duration, Instant};

pub(crate) fn validate_reference(reference: &str) -> Result<(), String> {
    let Some(path) = reference.strip_prefix("op://") else {
        return Err("secret reference must start with op://".into());
    };
    let parts: Vec<_> = path.split('/').collect();
    if !(3..=4).contains(&parts.len())
        || parts.iter().any(|p| p.is_empty())
        || !path
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b" /_-.%".contains(&b))
    {
        return Err("op reference must be op://vault/item/[section/]field using letters, digits, spaces, / _ - . or percent escapes".into());
    }
    Ok(())
}
pub(crate) fn resolve(
    references: &BTreeMap<String, String>,
) -> Result<Vec<(String, String)>, String> {
    resolve_with_op(references, Path::new("op"))
}
pub(crate) fn resolve_with_op(
    references: &BTreeMap<String, String>,
    executable: &Path,
) -> Result<Vec<(String, String)>, String> {
    if references.is_empty() {
        return Ok(Vec::new());
    }
    let unique: BTreeSet<_> = references.values().cloned().collect();
    for reference in &unique {
        validate_reference(reference)?;
    }
    let cancel = AtomicBool::new(false);
    let deadline = Instant::now() + Duration::from_secs(30);
    // --help is credential-free. Fail closed on old/missing CLI rather than doing
    // one field lookup per hook or guessing an unsupported batching API.
    let capability = crate::host_process::capture(
        Command::new(executable).args(["inject", "--help"]),
        Vec::new(),
        deadline,
        &cancel,
        64 * 1024,
        64 * 1024,
    )
    .map_err(|_| {
        "1Password CLI with `op inject` support is required; install/update op".to_owned()
    })?;
    capability
        .check_status()
        .map_err(|_| "op inject capability check failed; update op")?;
    let help = String::from_utf8_lossy(&capability.stdout);
    if !help.contains("--in-file") || !help.contains("--out-file") {
        return Err("installed op does not advertise supported inject template/stdout capabilities; update op".into());
    }
    // NUL separates fields unambiguously because NUL cannot be an environment value.
    // Input and output live only in bounded pipes/memory; no template files are written.
    let template: Vec<u8> = unique
        .iter()
        .flat_map(|r| format!("{{{{ {r} }}}}\0").into_bytes())
        .collect();
    let output = crate::host_process::capture(Command::new(executable).arg("inject"), template, deadline, &cancel, 1024 * 1024, 64 * 1024)
        .map_err(|_| "1Password batch injection failed, timed out or exceeded output limits; check host op authentication (values/diagnostics suppressed)".to_owned())?;
    output
        .check_status()
        .map_err(|_| "1Password batch injection failed (values/diagnostics suppressed)")?;
    let mut pieces = output.stdout.split(|b| *b == 0).collect::<Vec<_>>();
    if pieces.pop() != Some(&[][..]) || pieces.len() != unique.len() {
        return Err("1Password batch returned invalid field framing (values suppressed)".into());
    }
    let mut values = BTreeMap::new();
    for (reference, bytes) in unique.into_iter().zip(pieces) {
        let value = String::from_utf8(bytes.to_vec())
            .map_err(|_| "1Password field is not UTF-8 (value suppressed)")?;
        if value.contains(['\r', '\n', '\0']) {
            return Err("1Password field cannot contain NUL/newlines in environment transport (value suppressed)".into());
        }
        values.insert(reference, value);
    }
    references
        .iter()
        .map(|(key, reference)| {
            Ok((
                key.clone(),
                values
                    .get(reference)
                    .ok_or("missing resolved field")?
                    .clone(),
            ))
        })
        .collect()
}

#[cfg(test)]
#[path = "onepassword_refs_tests.rs"]
mod tests;
