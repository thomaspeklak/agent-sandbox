fn check_secrets(ck: &mut Checker, config: &ValidatedConfig) {
    ck.section("Secrets");
    let env_names: BTreeSet<&str> = config.secrets.iter().map(|s| s.env.as_str()).collect();
    if env_names.is_empty() {
        ck.warn("no secrets configured");
        return;
    }

    let command_runner = OsHostCommandRunner;
    let mut command_success = vec![None; config.secrets.len()];
    for (index, secret) in config.secrets.iter().enumerate() {
        let SecretSource::Command { argv } = &secret.source else {
            continue;
        };
        let executable = &argv[0];
        let label = format!("command secret helper for {}", secret.env);
        if !check_binary(ck, executable, &label, false) {
            command_success[index] = Some(false);
            continue;
        }
        match command_runner.lookup(argv, COMMAND_SECRET_TIMEOUT) {
            Ok(_) => {
                ck.ok(&format!(
                    "{} command lookup succeeded: {executable}",
                    secret.env
                ));
                command_success[index] = Some(true);
            }
            Err(error) => {
                ck.warn(&format!(
                    "{} command lookup failed: {executable} ({error})",
                    secret.env
                ));
                command_success[index] = Some(false);
            }
        }
    }

    for env_name in &env_names {
        if std::env::var(env_name).is_ok_and(|v| !v.is_empty()) {
            ck.ok(&format!("{env_name} available via environment"));
            continue;
        }
        let mut found = false;
        for (index, secret) in config
            .secrets
            .iter()
            .enumerate()
            .filter(|(_, secret)| secret.env == *env_name)
        {
            match &secret.source {
                SecretSource::Env { from_env } => {
                    if std::env::var(from_env).is_ok_and(|v| !v.is_empty()) {
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
                SecretSource::Command { .. } => {
                    if command_success[index] == Some(true) {
                        found = true;
                        break;
                    }
                }
            }
        }
        if !found {
            ck.warn(&format!("{env_name} not found in configured sources"));
        }
    }
}
