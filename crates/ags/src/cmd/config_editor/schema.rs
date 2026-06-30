use crate::config::DEFAULT_PI_SPEC;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScalarFieldKind {
    Text,
    Bool,
    Enum(&'static [&'static str]),
    Number { min: u64, max: u64 },
    StringList,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ScalarFieldSchema {
    pub key: &'static str,
    pub kind: ScalarFieldKind,
    pub required: bool,
    pub default_input: &'static str,
}

const SANDBOX_FIELDS: &[ScalarFieldSchema] = &[
    ScalarFieldSchema {
        key: "image",
        kind: ScalarFieldKind::Text,
        required: true,
        default_input: "localhost/agent-sandbox:latest",
    },
    ScalarFieldSchema {
        key: "containerfile",
        kind: ScalarFieldKind::Text,
        required: true,
        default_input: "~/.config/ags/Containerfile",
    },
    ScalarFieldSchema {
        key: "podman_network",
        kind: ScalarFieldKind::Enum(&["pasta", "slirp4netns"]),
        required: false,
        default_input: "pasta",
    },
    ScalarFieldSchema {
        key: "cache_dir",
        kind: ScalarFieldKind::Text,
        required: true,
        default_input: "~/.cache/ags",
    },
    ScalarFieldSchema {
        key: "gitconfig_path",
        kind: ScalarFieldKind::Text,
        required: true,
        default_input: "~/.config/ags/gitconfig-agent",
    },
    ScalarFieldSchema {
        key: "auth_key",
        kind: ScalarFieldKind::Text,
        required: true,
        default_input: "~/.ssh/ags-agent-auth",
    },
    ScalarFieldSchema {
        key: "sign_key",
        kind: ScalarFieldKind::Text,
        required: true,
        default_input: "~/.ssh/ags-agent-signing",
    },
    ScalarFieldSchema {
        key: "bootstrap_files",
        kind: ScalarFieldKind::StringList,
        required: false,
        default_input: "auth.json, models.json",
    },
    ScalarFieldSchema {
        key: "container_boot_dirs",
        kind: ScalarFieldKind::StringList,
        required: false,
        default_input: "/home/dev/.ssh",
    },
    ScalarFieldSchema {
        key: "passthrough_env",
        kind: ScalarFieldKind::StringList,
        required: false,
        default_input: "ANTHROPIC_API_KEY, OPENAI_API_KEY, GEMINI_API_KEY, OPENROUTER_API_KEY, AI_GATEWAY_API_KEY, OPENCODE_API_KEY",
    },
];

const BROWSER_FIELDS: &[ScalarFieldSchema] = &[
    ScalarFieldSchema {
        key: "enabled",
        kind: ScalarFieldKind::Bool,
        required: false,
        default_input: "false",
    },
    ScalarFieldSchema {
        key: "command",
        kind: ScalarFieldKind::Text,
        required: false,
        default_input: "",
    },
    ScalarFieldSchema {
        key: "profile_dir",
        kind: ScalarFieldKind::Text,
        required: false,
        default_input: "",
    },
    ScalarFieldSchema {
        key: "debug_port",
        kind: ScalarFieldKind::Number {
            min: 0,
            max: u16::MAX as u64,
        },
        required: false,
        default_input: "9222",
    },
    ScalarFieldSchema {
        key: "pi_skill_path",
        kind: ScalarFieldKind::Text,
        required: false,
        default_input: "",
    },
    ScalarFieldSchema {
        key: "command_args",
        kind: ScalarFieldKind::StringList,
        required: false,
        default_input: "",
    },
];

const AUTH_PROXY_FIELDS: &[ScalarFieldSchema] = &[ScalarFieldSchema {
    key: "auto_allow_domains",
    kind: ScalarFieldKind::StringList,
    required: false,
    default_input: "",
}];

const HOST_UI_FIELDS: &[ScalarFieldSchema] = &[
    ScalarFieldSchema {
        key: "enabled",
        kind: ScalarFieldKind::Bool,
        required: false,
        default_input: "false",
    },
    ScalarFieldSchema {
        key: "binary",
        kind: ScalarFieldKind::Text,
        required: false,
        default_input: "glimpse-host-ui",
    },
    ScalarFieldSchema {
        key: "renderer",
        kind: ScalarFieldKind::Text,
        required: false,
        default_input: "stub",
    },
    ScalarFieldSchema {
        key: "renderer_bin",
        kind: ScalarFieldKind::Text,
        required: false,
        default_input: "",
    },
    ScalarFieldSchema {
        key: "idle_timeout_ms",
        kind: ScalarFieldKind::Number {
            min: 0,
            max: u64::MAX,
        },
        required: false,
        default_input: "0",
    },
    ScalarFieldSchema {
        key: "log_level",
        kind: ScalarFieldKind::Enum(&["trace", "debug", "info", "warn", "error"]),
        required: false,
        default_input: "info",
    },
];

const CLIPBOARD_FIELDS: &[ScalarFieldSchema] = &[
    ScalarFieldSchema {
        key: "enabled",
        kind: ScalarFieldKind::Bool,
        required: false,
        default_input: "true",
    },
    ScalarFieldSchema {
        key: "mode",
        kind: ScalarFieldKind::Enum(&["off", "read", "readwrite"]),
        required: false,
        default_input: "readwrite",
    },
    ScalarFieldSchema {
        key: "max_bytes",
        kind: ScalarFieldKind::Number {
            min: 1,
            max: u64::MAX,
        },
        required: false,
        default_input: "33554432",
    },
    ScalarFieldSchema {
        key: "approval_required",
        kind: ScalarFieldKind::Bool,
        required: false,
        default_input: "true",
    },
    ScalarFieldSchema {
        key: "approval_seconds",
        kind: ScalarFieldKind::Number {
            min: 0,
            max: u64::MAX,
        },
        required: false,
        default_input: "300",
    },
    ScalarFieldSchema {
        key: "approve_writes",
        kind: ScalarFieldKind::Bool,
        required: false,
        default_input: "false",
    },
];

const DESKTOP_PASSTHROUGH_FIELDS: &[ScalarFieldSchema] = &[ScalarFieldSchema {
    key: "wayland",
    kind: ScalarFieldKind::Bool,
    required: false,
    default_input: "false",
}];

const PSP_FIELDS: &[ScalarFieldSchema] = &[ScalarFieldSchema {
    key: "binary",
    kind: ScalarFieldKind::Text,
    required: false,
    default_input: "",
}];

const UPDATE_FIELDS: &[ScalarFieldSchema] = &[
    ScalarFieldSchema {
        key: "pi_spec",
        kind: ScalarFieldKind::Text,
        required: false,
        default_input: DEFAULT_PI_SPEC,
    },
    ScalarFieldSchema {
        key: "minimum_release_age",
        kind: ScalarFieldKind::Number {
            min: 0,
            max: u32::MAX as u64,
        },
        required: false,
        default_input: "1440",
    },
];

pub fn scalar_fields(section_key: &str) -> &'static [ScalarFieldSchema] {
    match section_key {
        "sandbox" => SANDBOX_FIELDS,
        "browser" => BROWSER_FIELDS,
        "auth_proxy" => AUTH_PROXY_FIELDS,
        "host_ui" => HOST_UI_FIELDS,
        "clipboard" => CLIPBOARD_FIELDS,
        "desktop_passthrough" => DESKTOP_PASSTHROUGH_FIELDS,
        "psp" => PSP_FIELDS,
        "update" => UPDATE_FIELDS,
        _ => &[],
    }
}

pub fn scalar_field(section_key: &str, field_key: &str) -> Option<&'static ScalarFieldSchema> {
    scalar_fields(section_key)
        .iter()
        .find(|field| field.key == field_key)
}
