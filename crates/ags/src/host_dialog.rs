use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::Path;
use std::process::{Command, Output, Stdio};
use std::time::Duration;

use base64::Engine;
use serde_json::json;

const LOGO_WEBP: &[u8] = include_bytes!("../../../agent-sandbox-logo.webp");
const HOST_UI_HELLO_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DialogChoiceRole {
    Primary,
    Secondary,
    Cancel,
    Destructive,
}

impl DialogChoiceRole {
    fn css_class(self) -> &'static str {
        match self {
            Self::Primary => "primary",
            Self::Secondary => "secondary",
            Self::Cancel => "cancel",
            Self::Destructive => "destructive",
        }
    }
}

#[derive(Debug, Clone)]
pub struct DialogChoice {
    pub id: String,
    pub label: String,
    pub role: DialogChoiceRole,
}

impl DialogChoice {
    pub fn new(id: impl Into<String>, label: impl Into<String>, role: DialogChoiceRole) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            role,
        }
    }
}

#[derive(Debug, Clone)]
pub struct DialogDetail {
    pub label: String,
    pub value: String,
}

impl DialogDetail {
    pub fn new(label: impl Into<String>, value: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            value: value.into(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct DialogRequest {
    pub title: String,
    pub heading: String,
    pub message: String,
    pub details: Vec<DialogDetail>,
    pub note: Option<String>,
    pub choices: Vec<DialogChoice>,
    pub width: u32,
    pub height: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DialogOutcome {
    Choice(String),
    Cancelled,
    Unavailable,
}

pub fn prompt_choice(request: &DialogRequest, host_ui_socket: Option<&Path>) -> DialogOutcome {
    if let Some(socket_path) = host_ui_socket {
        match try_host_ui_prompt(request, socket_path) {
            Ok(outcome) => return outcome,
            Err(err) => eprintln!("[ags dialog] host UI prompt failed: {err}"),
        }
    }
    if let Some(outcome) = try_zenity(request) {
        return outcome;
    }
    if let Some(outcome) = try_kdialog(request) {
        return outcome;
    }
    DialogOutcome::Unavailable
}

fn try_host_ui_prompt(
    request: &DialogRequest,
    socket_path: &Path,
) -> Result<DialogOutcome, String> {
    let mut writer = UnixStream::connect(socket_path).map_err(|e| {
        format!(
            "failed to connect to host UI socket {}: {e}",
            socket_path.display()
        )
    })?;
    writer
        .set_read_timeout(Some(HOST_UI_HELLO_TIMEOUT))
        .map_err(|e| format!("failed to configure host UI socket: {e}"))?;
    let reader_stream = writer
        .try_clone()
        .map_err(|e| format!("failed to clone host UI socket: {e}"))?;
    let mut reader = BufReader::new(reader_stream);

    let _ = host_ui_request(
        &mut writer,
        &mut reader,
        "ags_dialog_hello",
        "hello",
        json!({
            "client_name": "ags-host-dialog",
            "client_version": env!("CARGO_PKG_VERSION"),
            "protocol_min": 1,
            "protocol_max": 1,
            "session_id": null,
        }),
    )?;

    writer
        .set_read_timeout(None)
        .map_err(|e| format!("failed to clear host UI timeout: {e}"))?;
    let result = host_ui_request(
        &mut writer,
        &mut reader,
        "ags_dialog_prompt",
        "prompt",
        json!({
            "source": { "kind": "html", "html": render_html(request) },
            "options": {
                "width": request.width,
                "height": request.height,
                "title": request.title,
            }
        }),
    )?;

    let value = result.get("value").unwrap_or(&serde_json::Value::Null);
    if value.is_null() {
        return Ok(DialogOutcome::Cancelled);
    }
    value
        .get("choice")
        .and_then(|choice| choice.as_str())
        .map(|choice| DialogOutcome::Choice(choice.to_owned()))
        .ok_or_else(|| "host UI prompt returned no choice".to_owned())
}

pub(crate) fn host_ui_request(
    writer: &mut UnixStream,
    reader: &mut BufReader<UnixStream>,
    id: &str,
    method: &str,
    params: serde_json::Value,
) -> Result<serde_json::Value, String> {
    let payload = json!({
        "v": 1,
        "kind": "request",
        "id": id,
        "method": method,
        "params": params,
    });
    writeln!(writer, "{payload}").map_err(|e| format!("failed to send host UI request: {e}"))?;
    writer
        .flush()
        .map_err(|e| format!("failed to flush host UI request: {e}"))?;

    let mut line = String::new();
    loop {
        line.clear();
        let read = reader
            .read_line(&mut line)
            .map_err(|e| format!("failed to read host UI response: {e}"))?;
        if read == 0 {
            return Err("host UI closed the connection".to_owned());
        }
        let value: serde_json::Value = serde_json::from_str(line.trim())
            .map_err(|e| format!("invalid host UI response: {e}"))?;

        if value.get("kind").and_then(|v| v.as_str()) != Some("response")
            || value.get("id").and_then(|v| v.as_str()) != Some(id)
        {
            continue;
        }

        return if value.get("ok").and_then(|v| v.as_bool()) == Some(true) {
            Ok(value
                .get("result")
                .cloned()
                .unwrap_or(serde_json::Value::Null))
        } else {
            let message = value
                .get("error")
                .and_then(|e| e.get("message"))
                .and_then(|v| v.as_str())
                .unwrap_or("host UI request failed");
            Err(message.to_owned())
        };
    }
}

fn render_html(request: &DialogRequest) -> String {
    let logo = base64::engine::general_purpose::STANDARD.encode(LOGO_WEBP);
    let details = request
        .details
        .iter()
        .map(|detail| {
            format!(
                r#"<div class="detail"><dt>{}</dt><dd>{}</dd></div>"#,
                escape_html(&detail.label),
                escape_html(&detail.value)
            )
        })
        .collect::<Vec<_>>()
        .join("");
    let details_html = if details.is_empty() {
        String::new()
    } else {
        format!(r#"<dl class="details">{details}</dl>"#)
    };
    let note_html = request
        .note
        .as_ref()
        .map(|note| format!(r#"<p class="note">{}</p>"#, escape_html(note)))
        .unwrap_or_default();
    let buttons = request
        .choices
        .iter()
        .map(|choice| {
            format!(
                r#"<button class="{}" data-choice="{}">{}</button>"#,
                choice.role.css_class(),
                escape_html(&choice.id),
                escape_html(&choice.label)
            )
        })
        .collect::<Vec<_>>()
        .join("");
    let cancel_choice = cancel_choice_id(request);

    format!(
        r#"<!doctype html>
<html>
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<style>
:root {{
  color-scheme: light dark; --bg: #f7f8fb; --panel: rgba(255, 255, 255, 0.9);
  --fg: #172033; --muted: #667085; --line: rgba(18, 28, 45, 0.12);
  --accent: #6d5dfc; --accent-hover: #5848e8; --danger: #dc2626;
  --shadow: 0 24px 70px rgba(25, 31, 54, 0.22);
}}
@media (prefers-color-scheme: dark) {{
  :root {{
    --bg: #0c111d; --panel: rgba(20, 27, 43, 0.92); --fg: #eef3ff;
    --muted: #98a2b3; --line: rgba(238, 243, 255, 0.12); --accent: #8b80ff;
    --accent-hover: #a39bff; --danger: #f87171; --shadow: 0 26px 80px rgba(0, 0, 0, 0.42);
  }}
}}
* {{ box-sizing: border-box; }}
body {{
  margin: 0; min-height: 100vh; display: grid; place-items: center; color: var(--fg);
  background: radial-gradient(circle at top left, rgba(109, 93, 252, 0.24), transparent 34%), var(--bg);
  font-family: Inter, ui-sans-serif, system-ui, -apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif;
}}
.dialog {{
  width: min(calc(100vw - 32px), 560px); background: var(--panel); padding: 24px;
  border: 1px solid var(--line); border-radius: 22px; box-shadow: var(--shadow);
}}
.brand {{ display: flex; gap: 14px; align-items: center; margin-bottom: 18px; }}
.logo {{ width: 48px; height: 48px; border-radius: 14px; box-shadow: 0 8px 20px rgba(109, 93, 252, 0.25); }}
.kicker {{ color: var(--muted); font-size: 12px; font-weight: 700; letter-spacing: 0.11em; text-transform: uppercase; }}
h1 {{ margin: 2px 0 0; font-size: 22px; line-height: 1.2; }}
.message {{ margin: 0; color: var(--muted); font-size: 14px; line-height: 1.55; }}
.details {{ margin: 18px 0 0; padding: 14px; border: 1px solid var(--line); border-radius: 16px; background: rgba(127, 127, 127, 0.06); }}
.detail + .detail {{ margin-top: 12px; padding-top: 12px; border-top: 1px solid var(--line); }}
dt {{ color: var(--muted); font-size: 11px; font-weight: 700; letter-spacing: 0.08em; text-transform: uppercase; }}
dd {{ margin: 5px 0 0; font-size: 13px; line-height: 1.45; word-break: break-word; }}
.note {{ margin: 14px 0 0; color: var(--muted); font-size: 12px; line-height: 1.45; }}
.actions {{ display: flex; justify-content: flex-end; gap: 10px; flex-wrap: wrap; margin-top: 22px; }}
button {{ border: 1px solid var(--line); border-radius: 999px; padding: 10px 16px; color: var(--fg); background: transparent; font: inherit; font-size: 13px; font-weight: 700; cursor: pointer; }}
button.primary {{ border-color: transparent; color: white; background: linear-gradient(135deg, var(--accent), var(--accent-hover)); }}
button.destructive {{ border-color: transparent; color: white; background: var(--danger); }}
button:focus-visible {{ outline: 3px solid color-mix(in srgb, var(--accent), transparent 65%); outline-offset: 2px; }}
@media (prefers-reduced-motion: no-preference) {{ button:hover {{ transform: translateY(-1px); }} }}
</style>
</head>
<body>
<main class="dialog" role="dialog" aria-modal="true" aria-labelledby="title">
  <header class="brand">
    <img class="logo" alt="AGS" src="data:image/webp;base64,{logo}">
    <div><div class="kicker">Agent Sandbox</div><h1 id="title">{heading}</h1></div>
  </header>
  <p class="message">{message}</p>
  {details_html}
  {note_html}
  <footer class="actions">{buttons}</footer>
</main>
<script>
const cancelChoice = {cancel_choice_json};
function choose(choice) {{ window.glimpse.send({{ choice }}); }}
document.querySelectorAll('button[data-choice]').forEach(button => {{
  button.addEventListener('click', () => choose(button.dataset.choice));
}});
document.addEventListener('keydown', event => {{
  if (event.key === 'Escape') choose(cancelChoice);
  if (event.key === 'Enter') {{
    const primary = document.querySelector('button.primary') || document.querySelector('button');
    if (primary) choose(primary.dataset.choice);
  }}
}});
</script>
</body>
</html>"#,
        heading = escape_html(&request.heading),
        message = escape_html(&request.message),
        cancel_choice_json =
            serde_json::to_string(&cancel_choice).unwrap_or_else(|_| "null".to_owned())
    )
}

fn try_zenity(request: &DialogRequest) -> Option<DialogOutcome> {
    let primary = first_non_cancel_choice(request)?;
    let cancel = cancel_choice(request);
    let text = fallback_text(request);
    let mut cmd = Command::new("zenity");
    cmd.args([
        "--question",
        "--title",
        &request.title,
        "--width",
        &request.width.to_string(),
        "--no-wrap",
        "--ok-label",
        &primary.label,
        "--cancel-label",
        &cancel.label,
        "--text",
        &text,
    ])
    .stdin(Stdio::null())
    .stderr(Stdio::null());

    for choice in request
        .choices
        .iter()
        .filter(|choice| choice.id != primary.id && choice.role != DialogChoiceRole::Cancel)
    {
        cmd.args(["--extra-button", &choice.label]);
    }

    let output = cmd.output().ok()?;
    Some(parse_zenity_output(
        request,
        &output,
        &primary.id,
        &cancel.id,
    ))
}

fn parse_zenity_output(
    request: &DialogRequest,
    output: &Output,
    primary_id: &str,
    cancel_id: &str,
) -> DialogOutcome {
    let stdout = String::from_utf8_lossy(&output.stdout);
    if let Some(choice) = choice_id_for_label(request, stdout.trim()) {
        return DialogOutcome::Choice(choice.to_owned());
    }
    if output.status.success() {
        DialogOutcome::Choice(primary_id.to_owned())
    } else if cancel_id.is_empty() {
        DialogOutcome::Cancelled
    } else {
        DialogOutcome::Choice(cancel_id.to_owned())
    }
}

fn try_kdialog(request: &DialogRequest) -> Option<DialogOutcome> {
    let primary = first_non_cancel_choice(request)?;
    let cancel = cancel_choice(request);
    let text = fallback_text(request);
    let mut cmd = Command::new("kdialog");
    cmd.arg("--title").arg(&request.title);

    let non_cancel_count = request
        .choices
        .iter()
        .filter(|choice| choice.role != DialogChoiceRole::Cancel)
        .count();
    if non_cancel_count <= 1 {
        cmd.args([
            "--yesno",
            &text,
            "--yes-label",
            &primary.label,
            "--no-label",
            &cancel.label,
        ]);
    } else {
        cmd.arg("--menu").arg(&text);
        for choice in &request.choices {
            cmd.arg(&choice.id).arg(&choice.label);
        }
    }

    let output = cmd
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .output()
        .ok()?;

    if !output.status.success() {
        return Some(DialogOutcome::Choice(cancel.id.clone()));
    }
    if non_cancel_count <= 1 {
        Some(DialogOutcome::Choice(primary.id.clone()))
    } else {
        let choice = String::from_utf8_lossy(&output.stdout).trim().to_owned();
        if request.choices.iter().any(|known| known.id == choice) {
            Some(DialogOutcome::Choice(choice))
        } else {
            Some(DialogOutcome::Choice(cancel.id.clone()))
        }
    }
}

fn fallback_text(request: &DialogRequest) -> String {
    let mut text = format!("{}\n\n{}", request.heading, request.message);
    for detail in &request.details {
        text.push_str(&format!("\n\n{}: {}", detail.label, detail.value));
    }
    if let Some(note) = &request.note {
        text.push_str(&format!("\n\n{note}"));
    }
    text
}

fn first_non_cancel_choice(request: &DialogRequest) -> Option<&DialogChoice> {
    request
        .choices
        .iter()
        .find(|choice| choice.role != DialogChoiceRole::Cancel)
}

fn cancel_choice(request: &DialogRequest) -> DialogChoice {
    request
        .choices
        .iter()
        .find(|choice| choice.role == DialogChoiceRole::Cancel)
        .cloned()
        .unwrap_or_else(|| DialogChoice::new("", "Cancel", DialogChoiceRole::Cancel))
}

fn cancel_choice_id(request: &DialogRequest) -> Option<String> {
    request
        .choices
        .iter()
        .find(|choice| choice.role == DialogChoiceRole::Cancel)
        .map(|choice| choice.id.clone())
}

fn choice_id_for_label<'a>(request: &'a DialogRequest, label: &str) -> Option<&'a str> {
    request
        .choices
        .iter()
        .find(|choice| choice.label.eq_ignore_ascii_case(label))
        .map(|choice| choice.id.as_str())
}

fn escape_html(input: &str) -> String {
    input
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

#[cfg(test)]
#[path = "host_dialog_tests.rs"]
mod tests;
