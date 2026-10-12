use super::stop_with_timeout;
use crate::t3::{
    registration::{Registration, Settings},
    repository::Repository,
};
use std::io::{self, Read};
use std::os::unix::net::UnixListener;
use std::time::Duration;

#[test]
fn a_stop_response_timeout_is_propagated_without_direct_container_fallback() {
    let root = tempfile::tempdir().unwrap();
    let registration = Registration {
        schema: 1,
        data_root: root.path().join("data"),
        control_dir: root.path().to_owned(),
        repository: Repository {
            id: "b".repeat(64),
            main: root.path().join("repo"),
            common: root.path().join("repo/.git"),
            worktrees: Vec::new(),
            device: 0,
            inode: 0,
        },
        config: root.path().join("config.toml"),
        overlay: None,
        home: root.path().join("home"),
        settings: Settings {
            browser: false,
            psp: false,
            psp_keep: false,
            yolo: false,
            root: false,
            wayland: false,
            add_dirs: Vec::new(),
            env_names: Vec::new(),
            op_sources: Vec::new(),
        },
    };
    let listener = UnixListener::bind(root.path().join("control.sock")).unwrap();
    let (finished, wait_for_timeout) = std::sync::mpsc::channel();
    let owner = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut request = String::new();
        stream.read_to_string(&mut request).unwrap();
        assert_eq!(request, "stop\n");
        wait_for_timeout.recv().unwrap();
    });
    let result = stop_with_timeout(&registration, Duration::from_millis(20));
    finished.send(()).unwrap();
    owner.join().unwrap();
    let error = result.unwrap_err();
    assert!(
        matches!(
            error.kind(),
            io::ErrorKind::TimedOut | io::ErrorKind::WouldBlock
        ),
        "{error}"
    );
}
