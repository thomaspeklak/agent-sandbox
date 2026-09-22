use ags::agent_runtime::{Update, selected};
use std::fs;
use std::io::{BufRead, BufReader, Read, Write};
use std::process::{Command, Stdio};

#[test]
fn running_node_can_lazily_import_old_dependency_after_update() {
    if Command::new("node").arg("--version").output().is_err() {
        eprintln!("skipping Node runtime regression: node is unavailable");
        return;
    }
    let temp = tempfile::tempdir().unwrap();
    let first = Update::begin(temp.path()).unwrap();
    let script = "console.log('ready'); process.stdin.once('data', async () => { console.log((await import('./dependency.mjs')).default); process.exit(0); });";
    fs::write(first.path.join("pnpm-home/main.mjs"), script).unwrap();
    fs::write(
        first.path.join("pnpm-home/dependency.mjs"),
        "export default 'old';",
    )
    .unwrap();
    first.publish().unwrap();
    let mut child = Command::new("node")
        .arg(selected(temp.path()).unwrap().join("pnpm-home/main.mjs"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let mut stdout = BufReader::new(child.stdout.take().unwrap());
    let mut line = String::new();
    stdout.read_line(&mut line).unwrap();
    assert_eq!(line, "ready\n");
    drop(first);
    let second = Update::begin(temp.path()).unwrap();
    fs::write(second.path.join("pnpm-home/main.mjs"), script).unwrap();
    fs::write(
        second.path.join("pnpm-home/dependency.mjs"),
        "export default 'new';",
    )
    .unwrap();
    second.publish().unwrap();
    writeln!(child.stdin.take().unwrap(), "load").unwrap();
    let mut loaded = String::new();
    stdout.read_to_string(&mut loaded).unwrap();
    assert!(child.wait().unwrap().success());
    assert_eq!(loaded, "old\n");
    let output = Command::new("node")
        .arg("--input-type=module")
        .arg("-e")
        .arg("console.log((await import('./dependency.mjs')).default)")
        .current_dir(selected(temp.path()).unwrap().join("pnpm-home"))
        .output()
        .unwrap();
    assert!(output.status.success());
    assert_eq!(output.stdout, b"new\n");
}
