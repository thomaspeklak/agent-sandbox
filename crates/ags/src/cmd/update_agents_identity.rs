use std::collections::BTreeMap;
use std::process::Command;

use crate::cli::Agent;
use crate::config::ValidatedConfig;

pub(super) fn image_id(image: &str) -> Result<String, String> {
    let output = Command::new("podman")
        .args(["image", "inspect", "--format", "{{.Id}}", image])
        .output()
        .map_err(|error| error.to_string())?;
    if !output.status.success() {
        return Err(format!(
            "cannot identify sandbox image; run ags update-image: {}",
            String::from_utf8_lossy(&output.stderr)
        ));
    }
    let id = String::from_utf8(output.stdout).map_err(|error| error.to_string())?;
    let id = id.trim();
    let digest = id.strip_prefix("sha256:").unwrap_or(id);
    if digest.len() != 64 || !digest.bytes().all(|c| c.is_ascii_hexdigit()) {
        return Err("Podman returned an invalid image ID".to_owned());
    }
    Ok(id.to_owned())
}

pub(super) fn request_identity(config: &ValidatedConfig, pi_spec: &str) -> serde_json::Value {
    let mut agents = BTreeMap::new();
    for agent in &config.sandbox.enabled_agents {
        let provider = if *agent == Agent::Pi {
            serde_json::json!({"type": "pnpm", "package": pi_spec})
        } else {
            config
                .sandbox
                .agent_providers
                .iter()
                .find(|p| p.agent == *agent)
                .map(|p| serde_json::to_value(&p.provider).expect("serializable provider"))
                .unwrap_or(serde_json::Value::Null)
        };
        agents.insert(agent.as_str(), provider);
    }
    serde_json::json!(agents)
}

pub(super) fn inventory_script(agents: &[Agent]) -> String {
    let agents = agents
        .iter()
        .map(|agent| agent.as_str())
        .collect::<Vec<_>>();
    let agents = serde_json::to_string(&agents).expect("serializable agent names");
    let snapshot = include_str!("update_agents_manifest.js");
    format!(
        r#"
node <<'AGS_RUNTIME_INVENTORY'
{snapshot}
const agents = {agents};
let dependencies = {{}};
if (agents.some(agent => agent === 'pi' || agent === 'gemini')) {{
  const output = require('node:child_process').execFileSync('/usr/local/bin/pnpm', ['list', '-g', '--depth=0', '--json'], {{
    env: {{ ...process.env, PNPM_HOME: '/usr/local/pnpm', PNPM_CONFIG_STORE_DIR: '/tmp/ags-pnpm-verification-store', PNPM_CONFIG_GLOBAL_BIN_DIR: '/usr/local/pnpm/bin' }},
    encoding: 'utf8'
  }});
  for (const project of JSON.parse(output)) Object.assign(dependencies, project.dependencies || {{}});
  if (!Object.keys(dependencies).length) throw new Error('pnpm runtime has no installed dependencies');
}}
const roots = {{
  'pnpm-home': '/usr/local/pnpm', 'codex-install': '/opt/codex-home',
  'claude-install': '/opt/claude-home', 'opencode-install': '/opt/opencode-home'
}};
fs.writeFileSync('/run/ags-update/inventory.json', JSON.stringify(snapshot(agents, dependencies, roots)));
AGS_RUNTIME_INVENTORY
"#
    )
}
