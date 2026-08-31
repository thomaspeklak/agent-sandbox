use std::fmt;

use super::CliError;

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, serde::Deserialize, serde::Serialize,
)]
#[serde(rename_all = "lowercase")]
pub enum Agent {
    Pi,
    Claude,
    Codex,
    Gemini,
    Opencode,
    Shell,
}

impl Agent {
    pub const INSTALLABLE: [Self; 5] = [
        Self::Pi,
        Self::Claude,
        Self::Codex,
        Self::Gemini,
        Self::Opencode,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Pi => "pi",
            Self::Claude => "claude",
            Self::Codex => "codex",
            Self::Gemini => "gemini",
            Self::Opencode => "opencode",
            Self::Shell => "shell",
        }
    }

    pub fn display_name(self) -> &'static str {
        match self {
            Self::Pi => "Pi",
            Self::Claude => "Claude Code",
            Self::Codex => "Codex",
            Self::Gemini => "Gemini CLI",
            Self::Opencode => "OpenCode",
            Self::Shell => "Shell",
        }
    }

    pub fn description(self) -> &'static str {
        match self {
            Self::Pi => "Extensible coding agent with AGS guard and browser integrations.",
            Self::Claude => "Anthropic's coding agent with AGS guard hook integration.",
            Self::Codex => "OpenAI's coding agent CLI.",
            Self::Gemini => "Google's Gemini coding agent CLI.",
            Self::Opencode => "Provider-agnostic terminal coding agent.",
            Self::Shell => "Interactive Bash shell without an agent CLI.",
        }
    }

    pub fn from_id(value: &str) -> Option<Self> {
        match value {
            "pi" => Some(Self::Pi),
            "claude" => Some(Self::Claude),
            "codex" => Some(Self::Codex),
            "gemini" => Some(Self::Gemini),
            "opencode" => Some(Self::Opencode),
            "shell" => Some(Self::Shell),
            _ => None,
        }
    }

    pub(super) fn parse(value: &str) -> Result<Self, CliError> {
        Self::from_id(value).ok_or_else(|| CliError::InvalidAgent(value.to_owned()))
    }
}

impl fmt::Display for Agent {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}
