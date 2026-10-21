//! Workdir selection regression tests; no real repository/hook approvals.
use super::*;

struct Projects {
    fixture: Fixture,
    current: std::path::PathBuf,
    selected: std::path::PathBuf,
}
impl Projects {
    fn new(trust_selected: bool) -> Self {
        let fixture = Fixture::new("#!/bin/sh\nexit 1\n");
        let current = fixture.root.path().join("work");
        let selected = fixture.root.path().join("other");
        fs::create_dir(&selected).unwrap();
        let global = fs::read_to_string(fixture.root.path().join("config.toml")).unwrap();
        for (repo, key) in [
            (&current, "CURRENT_PROJECT"),
            (&selected, "SELECTED_PROJECT"),
        ] {
            assert!(
                Command::new("git")
                    .args(["init", "--quiet"])
                    .current_dir(repo)
                    .status()
                    .unwrap()
                    .success()
            );
            fs::create_dir(repo.join(".ags")).unwrap();
            let hook = repo.join(".ags/fixture-hook");
            executable(
                &hook,
                &format!(
                    "#!/bin/sh\ncat > '{0}/input.json'\ntouch '{0}/executed'\nprintf '{{\"version\":1,\"env\":{{\"{key}\":\"synthetic\"}}}}'\n",
                    repo.display()
                ),
            );
            let overlay = repo.join(".ags/config.toml");
            fs::write(
                &overlay,
                global.replace(
                    fixture.root.path().join("fixture-hook").to_str().unwrap(),
                    hook.to_str().unwrap(),
                ),
            )
            .unwrap();
            fixture.approve_config(&overlay);
        }
        // Only overlay hooks should be selected; global config has no hook.
        fs::write(
            fixture.root.path().join("config.toml"),
            &global[global.find("[sandbox]").unwrap()..],
        )
        .unwrap();
        let config_dir = fixture.root.path().join("home/config/ags");
        fs::create_dir_all(&config_dir).unwrap();
        let trusted = if trust_selected {
            format!("{}\n{}\n", current.display(), selected.display())
        } else {
            format!("{}\n", current.display())
        };
        fs::write(config_dir.join("trusted-repo-overlays.txt"), trusted).unwrap();
        Self {
            fixture,
            current,
            selected,
        }
    }
    fn command(&self) -> Command {
        let mut command = self.fixture.command();
        command.args([
            "hooks",
            "test",
            "fixture",
            "--config",
            &self.fixture.config_arg(),
        ]);
        command
    }
}

#[test]
fn workdir_selects_named_hook_from_that_project_and_absence_preserves_cwd() {
    let projects = Projects::new(true);
    let output = projects
        .command()
        .args(["--workdir", "../other"])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(String::from_utf8_lossy(&output.stdout).contains("SELECTED_PROJECT"));
    assert!(!projects.current.join("executed").exists());
    let input: serde_json::Value =
        serde_json::from_slice(&fs::read(projects.selected.join("input.json")).unwrap()).unwrap();
    assert_eq!(input["project"], projects.selected.to_str().unwrap());
    assert_eq!(input["workdir"], projects.selected.to_str().unwrap());
    let output = projects.command().output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(String::from_utf8_lossy(&output.stdout).contains("CURRENT_PROJECT"));
    assert!(projects.current.join("executed").exists());
}

#[test]
fn workdir_untrusted_overlay_is_refused_instead_of_using_cwd_hook() {
    let projects = Projects::new(false);
    let output = projects
        .command()
        .args(["--workdir", projects.selected.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(!output.status.success());
    let error = String::from_utf8_lossy(&output.stderr);
    assert!(error.contains("overlay trust was not granted"), "{error}");
    assert!(!projects.current.join("executed").exists());
    assert!(!projects.selected.join("executed").exists());
}

#[test]
fn workdir_explicit_primary_overlay_is_not_loaded_twice_or_prompted_for_repo_trust() {
    let projects = Projects::new(false);
    let primary = projects.selected.join(".ags/config.toml");
    for spelling in [primary.to_str().unwrap(), "../other/.ags/config.toml"] {
        let output = projects
            .fixture
            .command()
            .args([
                "hooks",
                "test",
                "fixture",
                "--config",
                spelling,
                "--workdir",
                "../other",
            ])
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(String::from_utf8_lossy(&output.stdout).contains("SELECTED_PROJECT"));
        assert!(!projects.current.join("executed").exists());
    }
}
