mod bridge;
mod socket;

use std::io::{self, Write};
use std::process;

use clap::Parser;
use serde_json::{Value, json};

#[derive(Parser, Debug)]
#[command(name = "glimpse-shim")]
struct Args {
    #[arg(long, default_value_t = 800)]
    width: i32,
    #[arg(long, default_value_t = 600)]
    height: i32,
    #[arg(long, default_value = "Glimpse")]
    title: String,
    #[arg(long)]
    x: Option<i32>,
    #[arg(long)]
    y: Option<i32>,
    #[arg(long)]
    frameless: bool,
    #[arg(long)]
    floating: bool,
    #[arg(long)]
    transparent: bool,
    #[arg(long = "click-through")]
    click_through: bool,
    #[arg(long = "follow-cursor")]
    follow_cursor: bool,
    #[arg(long = "follow-mode", default_value = "snap")]
    follow_mode: String,
    #[arg(long = "cursor-anchor")]
    cursor_anchor: Option<String>,
    #[arg(long = "cursor-offset-x")]
    cursor_offset_x: Option<f64>,
    #[arg(long = "cursor-offset-y")]
    cursor_offset_y: Option<f64>,
    #[arg(long)]
    hidden: bool,
    #[arg(long = "auto-close")]
    auto_close: bool,
}

impl Args {
    fn to_window_options(&self) -> Value {
        let mut opts = json!({
            "width": self.width,
            "height": self.height,
            "title": self.title,
        });
        let m = opts.as_object_mut().unwrap();
        if let Some(x) = self.x {
            m.insert("x".into(), json!(x));
        }
        if let Some(y) = self.y {
            m.insert("y".into(), json!(y));
        }
        if self.frameless {
            m.insert("frameless".into(), json!(true));
        }
        if self.floating {
            m.insert("floating".into(), json!(true));
        }
        if self.transparent {
            m.insert("transparent".into(), json!(true));
        }
        if self.click_through {
            m.insert("click_through".into(), json!(true));
        }
        if self.follow_cursor {
            m.insert("follow_cursor".into(), json!(true));
        }
        m.insert("follow_mode".into(), json!(self.follow_mode));
        if let Some(ref anchor) = self.cursor_anchor {
            m.insert("cursor_anchor".into(), json!(anchor));
        }
        if self.cursor_offset_x.is_some() || self.cursor_offset_y.is_some() {
            m.insert(
                "cursor_offset".into(),
                json!({ "x": self.cursor_offset_x, "y": self.cursor_offset_y }),
            );
        }
        if self.hidden {
            m.insert("hidden".into(), json!(true));
        }
        if self.auto_close {
            m.insert("auto_close".into(), json!(true));
        }
        opts
    }
}

fn main() {
    if let Err(err) = run() {
        eprintln!("[glimpse-shim] {err}");
        process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();

    let sock_path = std::env::var("AGS_HOST_UI_SOCK")
        .unwrap_or_else(|_| "/run/ags-host-ui/host-ui.sock".to_string());

    let mut conn = socket::SocketConn::connect(&sock_path)?;
    conn.hello()?;

    // Emit synthetic ready so the JS wrapper sends source.
    emit_stdout(&json!({
        "type": "ready",
        "screen": {},
        "screens": [],
        "appearance": {},
        "cursor": { "x": 0, "y": 0 },
        "cursorTip": null
    }));

    let options = args.to_window_options();
    bridge::run(conn, options)
}

pub fn emit_stdout(value: &Value) {
    let line = serde_json::to_string(value).unwrap();
    let mut out = io::stdout().lock();
    let _ = writeln!(out, "{line}");
    let _ = out.flush();
}
