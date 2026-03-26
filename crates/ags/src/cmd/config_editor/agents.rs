/// Shared agent mount definitions used by both the config editor TUI and
/// the `ags install --add-agent-mounts` command.
use crate::config::MountKind;

pub struct AgentMountDef {
    pub host: &'static str,
    pub container: &'static str,
    pub kind: MountKind,
}

pub struct AgentDef {
    pub name: &'static str,
    pub mounts: &'static [AgentMountDef],
}

pub const KNOWN_AGENTS: &[AgentDef] = &[
    AgentDef {
        name: "Claude",
        mounts: &[
            AgentMountDef {
                host: "~/.claude.json",
                container: "/home/dev/.claude.json",
                kind: MountKind::File,
            },
            AgentMountDef {
                host: "~/.claude",
                container: "/home/dev/.claude",
                kind: MountKind::Dir,
            },
        ],
    },
    AgentDef {
        name: "Codex",
        mounts: &[AgentMountDef {
            host: "~/.codex",
            container: "/home/dev/.codex",
            kind: MountKind::Dir,
        }],
    },
    AgentDef {
        name: "Pi",
        mounts: &[AgentMountDef {
            host: "~/.pi",
            container: "/home/dev/.pi",
            kind: MountKind::Dir,
        }],
    },
    AgentDef {
        name: "Opencode",
        mounts: &[AgentMountDef {
            host: "~/.config/opencode",
            container: "/home/dev/.config/opencode",
            kind: MountKind::Dir,
        }],
    },
    AgentDef {
        name: "Gemini",
        mounts: &[AgentMountDef {
            host: "~/.gemini",
            container: "/home/dev/.gemini",
            kind: MountKind::Dir,
        }],
    },
];
