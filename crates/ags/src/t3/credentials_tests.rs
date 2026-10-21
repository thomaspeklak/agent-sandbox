use super::sealed_environment;
use std::collections::HashMap;

#[cfg(target_os = "linux")]
#[test]
fn boot_environment_is_rewound_and_sealed_against_mutation() {
    use std::fs::File;
    use std::io::{Seek, Write};
    use std::os::fd::AsRawFd;

    let environment = HashMap::from([
        ("T3_TEST_SECRET".to_owned(), "fixture-value".to_owned()),
        (
            "UNICODE".to_owned(),
            "a\nquoted \"value\" with 🦀".to_owned(),
        ),
    ]);
    let mut file = File::from(sealed_environment(&environment).unwrap());
    assert_eq!(file.stream_position().unwrap(), 0);
    let actual: HashMap<String, String> = serde_json::from_reader(&mut file).unwrap();
    assert_eq!(actual, environment);
    assert_eq!(
        unsafe { libc::fcntl(file.as_raw_fd(), libc::F_GET_SEALS) },
        libc::F_SEAL_WRITE | libc::F_SEAL_GROW | libc::F_SEAL_SHRINK | libc::F_SEAL_SEAL
    );
    file.rewind().unwrap();
    assert_eq!(
        file.write_all(b"mutation").unwrap_err().raw_os_error(),
        Some(libc::EPERM)
    );
    assert_eq!(
        file.set_len(0).unwrap_err().raw_os_error(),
        Some(libc::EPERM)
    );
    assert_eq!(
        file.set_len(4096).unwrap_err().raw_os_error(),
        Some(libc::EPERM)
    );
}

#[cfg(not(target_os = "linux"))]
#[test]
fn boot_environment_reports_unsupported_on_non_linux_hosts() {
    let error = sealed_environment(&HashMap::new()).unwrap_err();
    assert_eq!(error.kind(), std::io::ErrorKind::Unsupported);
    assert_eq!(
        error.to_string(),
        "sealed T3 boot descriptors require Linux"
    );
}
