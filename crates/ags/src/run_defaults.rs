use crate::cli::Agent;

const CLAUDE_PASSTHROUGH_DEFAULTS: &[&str] =
    &["--strict-mcp-config", "--dangerously-skip-permissions"];
const GEMINI_PASSTHROUGH_DEFAULTS: &[&str] = &["--yolo"];

pub fn passthrough_args(agent: Agent) -> &'static [&'static str] {
    match agent {
        Agent::Claude => CLAUDE_PASSTHROUGH_DEFAULTS,
        Agent::Gemini => GEMINI_PASSTHROUGH_DEFAULTS,
        Agent::Pi | Agent::Codex | Agent::Opencode | Agent::Shell => &[],
    }
}

pub fn prepend_passthrough_args(agent: Agent, args: &mut Vec<String>) {
    let defaults = passthrough_args(agent);
    if defaults.is_empty() {
        return;
    }

    let mut merged: Vec<String> = defaults.iter().map(|arg| (*arg).to_owned()).collect();
    merged.append(args);
    *args = merged;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn claude_defaults_include_shortcut_passthrough_args() {
        assert_eq!(
            passthrough_args(Agent::Claude),
            &["--strict-mcp-config", "--dangerously-skip-permissions"]
        );
    }

    #[test]
    fn prepend_passthrough_args_keeps_user_args_last() {
        let mut args = vec!["--model".to_owned(), "opus".to_owned()];
        prepend_passthrough_args(Agent::Claude, &mut args);
        assert_eq!(
            args,
            vec![
                "--strict-mcp-config".to_owned(),
                "--dangerously-skip-permissions".to_owned(),
                "--model".to_owned(),
                "opus".to_owned()
            ]
        );
    }

    #[test]
    fn agents_without_defaults_are_unchanged() {
        let mut args = vec!["--continue".to_owned()];
        prepend_passthrough_args(Agent::Pi, &mut args);
        assert_eq!(args, vec!["--continue".to_owned()]);
    }
}
