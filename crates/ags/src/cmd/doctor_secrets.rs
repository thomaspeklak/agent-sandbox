use std::collections::BTreeSet;

use crate::config::{SecretSource, ValidatedConfig};
use crate::secrets::{COMMAND_SECRET_TIMEOUT, HostCommandRunner, OsHostCommandRunner};

use super::check_binary;
use crate::cmd::doctor_util::{Checker, secret_tool_has_value};

pub(super) fn check_secrets(ck: &mut Checker, config: &ValidatedConfig) {
    ck.section("Secrets");
    let env_names: BTreeSet<&str> = config.secrets.iter().map(|s| s.env.as_str()).collect();
    if env_names.is_empty() {
        ck.warn("no secrets configured");
        return;
    }

    let command_runner = OsHostCommandRunner;
    for env_name in &env_names {
        if std::env::var(env_name).is_ok_and(|value| !value.is_empty()) {
            ck.ok(&format!("{env_name} available via environment"));
            continue;
        }

        let mut found = false;
        for secret in config
            .secrets
            .iter()
            .filter(|secret| secret.env == *env_name)
        {
            match &secret.source {
                SecretSource::Env { from_env } => {
                    if std::env::var(from_env).is_ok_and(|value| !value.is_empty()) {
                        ck.ok(&format!(
                            "{env_name} available via source env var: {from_env}"
                        ));
                        found = true;
                        break;
                    }
                }
                SecretSource::SecretTool { attributes } => {
                    if secret_tool_has_value(attributes) {
                        ck.ok(&format!("{env_name} found in keyring"));
                        found = true;
                        break;
                    }
                }
                SecretSource::Command { argv } => {
                    let executable = &argv[0];
                    let label = format!("command secret helper for {env_name}");
                    if !check_binary(ck, executable, &label, false) {
                        continue;
                    }
                    match command_runner.lookup(argv, COMMAND_SECRET_TIMEOUT) {
                        Ok(_) => {
                            ck.ok(&format!(
                                "{env_name} command lookup succeeded: {executable}"
                            ));
                            found = true;
                            break;
                        }
                        Err(error) => ck.warn(&format!(
                            "{env_name} command lookup failed: {executable} ({error})"
                        )),
                    }
                }
            }
        }
        if !found {
            ck.warn(&format!("{env_name} not found in configured sources"));
        }
    }
}
