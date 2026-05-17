use super::*;
use std::os::unix::process::ExitStatusExt;

fn request() -> DialogRequest {
    DialogRequest {
        title: "Test".to_owned(),
        heading: "Allow?".to_owned(),
        message: "Choose".to_owned(),
        details: vec![DialogDetail::new("Host", "example.com")],
        note: None,
        choices: vec![
            DialogChoice::new("open", "Open", DialogChoiceRole::Primary),
            DialogChoice::new("proxy", "Proxy", DialogChoiceRole::Secondary),
            DialogChoice::new("cancel", "Cancel", DialogChoiceRole::Cancel),
        ],
        width: 420,
        height: 260,
    }
}

#[test]
fn parses_zenity_extra_button_from_stdout() {
    let output = Output {
        status: std::process::ExitStatus::from_raw(256),
        stdout: b"Proxy\n".to_vec(),
        stderr: Vec::new(),
    };
    assert_eq!(
        parse_zenity_output(&request(), &output, "open", "cancel"),
        DialogOutcome::Choice("proxy".to_owned())
    );
}

#[test]
fn html_escapes_dynamic_content() {
    let mut request = request();
    request.details = vec![DialogDetail::new("URL", "https://x.test/?a=1&b=<tag>")];
    let html = render_html(&request);
    assert!(html.contains("&amp;b=&lt;tag&gt;"));
    assert!(!html.contains("?a=1&b=<tag>"));
}

#[test]
fn fallback_text_contains_details_and_note() {
    let mut request = request();
    request.note = Some("Be careful".to_owned());
    let text = fallback_text(&request);
    assert!(text.contains("Host: example.com"));
    assert!(text.contains("Be careful"));
}
