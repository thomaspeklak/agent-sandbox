use std::fs;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use base64::Engine;
use sha2::{Digest, Sha512};

struct Registry {
    address: String,
    stop: Arc<AtomicBool>,
    downloads: Arc<AtomicUsize>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl Registry {
    fn start(tarball: PathBuf) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let address = listener.local_addr().unwrap().to_string();
        let stop = Arc::new(AtomicBool::new(false));
        let downloads = Arc::new(AtomicUsize::new(0));
        let thread_stop = stop.clone();
        let thread_downloads = downloads.clone();
        let url = format!("http://{address}/ags-cache-fixture/-/ags-cache-fixture-1.0.0.tgz");
        let archive = fs::read(tarball).unwrap();
        let integrity = format!(
            "sha512-{}",
            base64::engine::general_purpose::STANDARD.encode(Sha512::digest(&archive))
        );
        let metadata = serde_json::to_vec(&serde_json::json!({
            "name": "ags-cache-fixture",
            "dist-tags": {"latest": "1.0.0"},
            "versions": {"1.0.0": {
                "name": "ags-cache-fixture", "version": "1.0.0",
                "dist": {"tarball": url, "integrity": integrity}
            }}
        }))
        .unwrap();
        let thread = std::thread::spawn(move || {
            while !thread_stop.load(Ordering::Relaxed) {
                match listener.accept() {
                    Ok((mut stream, _)) => {
                        serve(&mut stream, &metadata, &archive, &thread_downloads)
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        std::thread::sleep(std::time::Duration::from_millis(5));
                    }
                    Err(error) => panic!("registry accept failed: {error}"),
                }
            }
        });
        Self {
            address,
            stop,
            downloads,
            thread: Some(thread),
        }
    }

    fn url(&self) -> String {
        format!("http://{}/", self.address)
    }
}

impl Drop for Registry {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        let _ = TcpStream::connect(&self.address);
        if let Some(thread) = self.thread.take() {
            thread.join().unwrap();
        }
    }
}

fn serve(stream: &mut TcpStream, metadata: &[u8], archive: &[u8], downloads: &AtomicUsize) {
    let mut request = [0u8; 8192];
    let count = stream.read(&mut request).unwrap();
    let first = String::from_utf8_lossy(&request[..count])
        .lines()
        .next()
        .unwrap_or("")
        .to_owned();
    let (status, content_type, body) = if first.contains("/ags-cache-fixture/-/") {
        downloads.fetch_add(1, Ordering::Relaxed);
        ("200 OK", "application/octet-stream", archive)
    } else if first.starts_with("GET /ags-cache-fixture ") {
        ("200 OK", "application/json", metadata)
    } else {
        ("200 OK", "application/json", b"{}" as &[u8])
    };
    write!(stream, "HTTP/1.1 {status}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", body.len()).unwrap();
    stream.write_all(body).unwrap();
}

fn pnpm(
    project: &Path,
    home: &Path,
    store: &Path,
    cache: &Path,
    registry: &str,
    offline: bool,
) -> std::process::Output {
    let mut command = Command::new("sh");
    command.args(["-c", "exec pnpm \"$@\"", "sh", "add"]);
    if offline {
        command.arg("--offline");
    }
    command
        .arg("--ignore-scripts")
        .arg("ags-cache-fixture@1.0.0")
        .current_dir(project)
        .env("HOME", home)
        .env("PNPM_HOME", home.join("pnpm-global"))
        .env("PNPM_CONFIG_GLOBAL_BIN_DIR", home.join("pnpm-global"))
        .env("PNPM_CONFIG_STORE_DIR", store)
        .env("PNPM_CONFIG_CACHE_DIR", cache)
        .env("PNPM_CONFIG_PACKAGE_IMPORT_METHOD", "clone-or-copy")
        .env("PNPM_CONFIG_VIRTUAL_STORE_TYPE", "project")
        .env("PNPM_CONFIG_ENABLE_GLOBAL_VIRTUAL_STORE", "false")
        .env("PNPM_CONFIG_VERIFY_STORE_INTEGRITY", "true")
        .env("PNPM_CONFIG_SIDE_EFFECTS_CACHE", "false")
        .env("PNPM_CONFIG_REGISTRY", registry)
        .env("PNPM_CONFIG_AUDIT", "false")
        .env("PNPM_CONFIG_UPDATE_NOTIFIER", "false")
        .output()
        .unwrap()
}

#[test]
fn development_store_is_writable_reusable_and_independent_of_read_only_runtime() {
    match Command::new("sh").args(["-c", "pnpm --version"]).output() {
        Ok(output) if output.status.success() => {}
        Ok(output) => {
            eprintln!(
                "skipping pnpm storage integration: pnpm exited with {}",
                output.status
            );
            return;
        }
        Err(error) => {
            eprintln!("skipping pnpm storage integration: {error}");
            return;
        }
    }
    let temp = tempfile::tempdir().unwrap();
    let package = temp.path().join("source/package");
    fs::create_dir_all(&package).unwrap();
    fs::write(
        package.join("package.json"),
        r#"{"name":"ags-cache-fixture","version":"1.0.0","main":"index.js"}"#,
    )
    .unwrap();
    fs::write(package.join("index.js"), "module.exports = 'cached';\n").unwrap();
    let tarball = temp.path().join("fixture.tgz");
    assert!(
        Command::new("tar")
            .args([
                "-czf",
                tarball.to_str().unwrap(),
                "-C",
                temp.path().join("source").to_str().unwrap(),
                "package"
            ])
            .status()
            .unwrap()
            .success()
    );
    let registry = Registry::start(tarball);
    let runtime = temp.path().join("generation/pnpm-home");
    fs::create_dir_all(&runtime).unwrap();
    let runtime_file = runtime.join("agent.js");
    fs::write(&runtime_file, "immutable agent runtime").unwrap();
    fs::set_permissions(&runtime, fs::Permissions::from_mode(0o555)).unwrap();
    let home = temp.path().join("home");
    let store = temp.path().join("workspace-cache/store");
    let cache = temp.path().join("workspace-cache/cache");
    for path in [&home, &store, &cache] {
        fs::create_dir_all(path).unwrap();
    }

    let first = temp.path().join("project-a");
    let second = temp.path().join("project-b");
    fs::create_dir(&first).unwrap();
    fs::create_dir(&second).unwrap();
    let output = pnpm(&first, &home, &store, &cache, &registry.url(), false);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let output = pnpm(&second, &home, &store, &cache, &registry.url(), true);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(registry.downloads.load(Ordering::Relaxed), 1);
    assert_eq!(
        fs::read_to_string(&runtime_file).unwrap(),
        "immutable agent runtime"
    );
    assert!(!runtime.join(".store").exists());
    let installed =
        fs::canonicalize(second.join("node_modules/ags-cache-fixture/index.js")).unwrap();
    assert!(installed.starts_with(&second));
    let installed_meta = fs::metadata(&installed).unwrap();
    let cached_copy = walk_files(&store)
        .into_iter()
        .find(|path| {
            fs::read_to_string(path).ok().as_deref() == Some("module.exports = 'cached';\n")
        })
        .unwrap();
    assert_ne!(
        installed_meta.ino(),
        fs::metadata(&cached_copy).unwrap().ino()
    );
    let cached_before = fs::read(&cached_copy).unwrap();
    fs::write(&installed, "module.exports = 'mutated';\n").unwrap();
    assert_eq!(fs::read(cached_copy).unwrap(), cached_before);
}

fn walk_files(root: &Path) -> Vec<PathBuf> {
    let mut pending = vec![root.to_owned()];
    let mut files = Vec::new();
    while let Some(path) = pending.pop() {
        for entry in fs::read_dir(path).unwrap() {
            let entry = entry.unwrap();
            if entry.file_type().unwrap().is_dir() {
                pending.push(entry.path());
            } else if entry.file_type().unwrap().is_file() {
                files.push(entry.path());
            }
        }
    }
    files
}
