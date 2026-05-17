use super::*;
use std::sync::Mutex;

#[derive(Default)]
struct MockBackend {
    writes: Mutex<Vec<(String, Vec<u8>)>>,
}

impl ClipboardBackend for MockBackend {
    fn list_types(&self) -> Result<Vec<String>, String> {
        Ok(vec!["text/plain".to_owned(), "image/png".to_owned()])
    }

    fn read(&self, mime: Option<&str>, _max_bytes: usize) -> Result<(String, Vec<u8>), String> {
        Ok((mime.unwrap_or("text/plain").to_owned(), b"hello".to_vec()))
    }

    fn write(&self, mime: &str, data: &[u8]) -> Result<(), String> {
        self.writes
            .lock()
            .unwrap()
            .push((mime.to_owned(), data.to_vec()));
        Ok(())
    }
}

struct DenyClipboardAccess;

impl ClipboardAccessAuthorizer for DenyClipboardAccess {
    fn authorize(&self, _operation: ClipboardOperation, _mime: Option<&str>) -> Result<(), String> {
        Err("denied for test".to_owned())
    }
}

#[test]
fn read_request_returns_base64_payload() {
    let backend = MockBackend::default();
    let response = handle_request(
        &json!({"op":"read", "mime":"text/plain"}),
        ClipboardMode::Read,
        1024,
        &AllowAllClipboardAccess,
        &backend,
    );
    assert_eq!(response["ok"], true);
    assert_eq!(response["data_b64"], "aGVsbG8=");
}

#[test]
fn read_request_respects_approval() {
    let backend = MockBackend::default();
    let response = handle_request(
        &json!({"op":"read", "mime":"text/plain"}),
        ClipboardMode::Read,
        1024,
        &DenyClipboardAccess,
        &backend,
    );
    assert_eq!(response["ok"], false);
    assert!(response["error"].as_str().unwrap().contains("denied"));
}

#[test]
fn write_request_respects_mode() {
    let backend = MockBackend::default();
    let response = handle_request(
        &json!({"op":"write", "mime":"text/plain", "data_b64":"aGk="}),
        ClipboardMode::Read,
        1024,
        &AllowAllClipboardAccess,
        &backend,
    );
    assert_eq!(response["ok"], false);
    assert!(
        response["error"]
            .as_str()
            .unwrap()
            .contains("write disabled")
    );
}

#[test]
fn oversized_write_is_rejected() {
    let backend = MockBackend::default();
    let response = handle_request(
        &json!({"op":"write", "data_b64":"aGVsbG8="}),
        ClipboardMode::ReadWrite,
        2,
        &AllowAllClipboardAccess,
        &backend,
    );
    assert_eq!(response["ok"], false);
    assert!(response["error"].as_str().unwrap().contains("above limit"));
}

#[test]
fn formats_approval_window_duration() {
    assert_eq!(format_duration(300), "5 minutes");
    assert_eq!(format_duration(1), "1 second");
    assert_eq!(format_duration(7200), "2 hours");
}
