use std::fs;
use std::process::Command;

use super::NODE_WRAPPER_SETUP;

#[test]
fn node_wrapper_setup_is_valid_bash() {
    let status = Command::new("bash")
        .args(["-n", "-c", NODE_WRAPPER_SETUP])
        .status()
        .unwrap();
    assert!(status.success());
}

#[test]
fn node_wrapper_uses_offline_mise_and_exact_remediation() {
    assert!(NODE_WRAPPER_SETUP.contains("mise --no-config --offline where"));
    assert!(NODE_WRAPPER_SETUP.contains("ags node install %q"));
    assert!(NODE_WRAPPER_SETUP.contains("AGS_NODE_WORKSPACE_ROOT"));
    assert!(NODE_WRAPPER_SETUP.contains("$node_root/bin/$name"));
    assert!(NODE_WRAPPER_SETUP.contains("AGS_NODE_AGENT_BOOTSTRAP"));
}

#[test]
fn agent_bootstrap_marker_uses_baseline_node_once() {
    let temp = tempfile::tempdir().unwrap();
    let baseline = temp.path().join("baseline-node");
    fs::write(&baseline, "#!/bin/sh\nprintf 'baseline\\n'\n").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&baseline, fs::Permissions::from_mode(0o755)).unwrap();
    }
    let wrapper_dir = temp.path().join("wrappers");
    let setup = NODE_WRAPPER_SETUP
        .replace("/home/dev/.local/bin", &wrapper_dir.display().to_string())
        .replace("/usr/bin/node", &baseline.display().to_string());
    fs::write(temp.path().join("setup.sh"), setup).unwrap();
    assert!(
        Command::new("bash")
            .arg(temp.path().join("setup.sh"))
            .status()
            .unwrap()
            .success()
    );

    let output = Command::new(wrapper_dir.join("node"))
        .env("AGS_NODE_AGENT_BOOTSTRAP", "1")
        .output()
        .unwrap();
    assert!(output.status.success());
    assert_eq!(String::from_utf8(output.stdout).unwrap(), "baseline\n");
}

#[test]
fn npm_and_its_node_child_use_the_selected_runtime() {
    let temp = tempfile::tempdir().unwrap();
    let bin = temp.path().join("bin");
    let node_root = temp.path().join("node");
    let workspace = temp.path().join("workspace");
    fs::create_dir_all(node_root.join("bin")).unwrap();
    fs::create_dir_all(&workspace).unwrap();
    fs::create_dir_all(&bin).unwrap();
    fs::write(workspace.join(".nvmrc"), "22\n").unwrap();
    fs::write(
        bin.join("mise"),
        format!("#!/bin/sh\nprintf '%s\\n' {}\n", node_root.display()),
    )
    .unwrap();
    fs::write(
        node_root.join("bin/node"),
        r#"#!/bin/sh
printf 'selected-node:%s\n' "$*"
"#,
    )
    .unwrap();
    fs::write(node_root.join("bin/npm"), "#!/bin/sh\nnode from-npm\n").unwrap();
    for file in [
        bin.join("mise"),
        node_root.join("bin/node"),
        node_root.join("bin/npm"),
    ] {
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&file, fs::Permissions::from_mode(0o755)).unwrap();
        }
    }

    let wrapper_dir = temp.path().join("wrappers");
    let setup =
        NODE_WRAPPER_SETUP.replace("/home/dev/.local/bin", &wrapper_dir.display().to_string());
    fs::write(temp.path().join("setup.sh"), setup).unwrap();
    let output = Command::new("bash")
        .arg(temp.path().join("setup.sh"))
        .current_dir(&workspace)
        .env("PATH", format!("{}:/usr/bin:/bin", bin.display()))
        .env("AGS_NODE_WORKSPACE_ROOT", &workspace)
        .env("MISE_DATA_DIR", temp.path().join("store"))
        .output()
        .unwrap();
    assert!(output.status.success());

    let output = Command::new(wrapper_dir.join("npm"))
        .current_dir(&workspace)
        .env(
            "PATH",
            format!("{}:{}:/usr/bin:/bin", wrapper_dir.display(), bin.display()),
        )
        .env("AGS_NODE_WORKSPACE_ROOT", &workspace)
        .env("MISE_DATA_DIR", temp.path().join("store"))
        .output()
        .unwrap();
    assert!(output.status.success());
    assert_eq!(
        String::from_utf8(output.stdout).unwrap(),
        "selected-node:from-npm\n"
    );

    // A native agent launcher has no bootstrap marker to consume. Its first
    // Node child must therefore select the project's managed runtime.
    let native_agent = temp.path().join("native-agent");
    fs::write(&native_agent, "#!/bin/sh\nnode from-native-agent\n").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&native_agent, fs::Permissions::from_mode(0o755)).unwrap();
    }
    let output = Command::new(native_agent)
        .current_dir(&workspace)
        .env(
            "PATH",
            format!("{}:{}:/usr/bin:/bin", wrapper_dir.display(), bin.display()),
        )
        .env("AGS_NODE_WORKSPACE_ROOT", &workspace)
        .env("MISE_DATA_DIR", temp.path().join("store"))
        .output()
        .unwrap();
    assert!(output.status.success());
    assert_eq!(
        String::from_utf8(output.stdout).unwrap(),
        "selected-node:from-native-agent\n"
    );
}
