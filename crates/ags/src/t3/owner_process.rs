//! Recover the old owner's process group without signalling a reused host PID.
use super::registration::Registration;
use serde::{Deserialize, Serialize};
use std::fs;
use std::io;
use std::time::{Duration, Instant};

#[derive(Serialize, Deserialize)]
struct Stamp {
    pid: i32,
    start: String,
}

fn process_fields(pid: i32) -> io::Result<String> {
    let stat = fs::read_to_string(format!("/proc/{pid}/stat"))?;
    stat.rsplit_once(')')
        .map(|(_, fields)| fields.trim().to_owned())
        .ok_or_else(|| io::Error::other("invalid owner process stat"))
}

pub fn record(registration: &Registration) -> io::Result<()> {
    let pid = std::process::id() as i32;
    let start = process_fields(pid)?
        .split_whitespace()
        .nth(19)
        .ok_or_else(|| io::Error::other("owner process start time missing"))?
        .to_owned();
    super::registration::write_json(
        &registration.private_dir()?.join("owner-process.json"),
        &Stamp { pid, start },
    )
}

fn group_alive(group: i32) -> bool {
    fs::read_dir("/proc").is_ok_and(|entries| {
        entries.flatten().any(|entry| {
            let Some(pid) = entry
                .file_name()
                .to_str()
                .and_then(|name| name.parse::<i32>().ok())
            else {
                return false;
            };
            let Ok(fields) = process_fields(pid) else {
                return false;
            };
            let mut fields = fields.split_whitespace();
            fields.next() != Some("Z")
                && fields.nth(1).and_then(|group| group.parse::<i32>().ok()) == Some(group)
        })
    })
}

pub fn recover(registration: &Registration) -> io::Result<()> {
    let path = registration.private_dir()?.join("owner-process.json");
    if !path.exists() {
        return Ok(());
    }
    let stamp: Stamp = serde_json::from_slice(&fs::read(path)?)?;
    if stamp.pid < 2 || stamp.pid == std::process::id() as i32 {
        return Err(io::Error::other(
            "invalid previous T3 owner process identity",
        ));
    }
    if let Ok(fields) = process_fields(stamp.pid)
        && fields.split_whitespace().nth(19) != Some(stamp.start.as_str())
    {
        return Err(io::Error::other(
            "previous T3 owner PID was reused; inspect leftover services before restarting",
        ));
    }
    unsafe {
        libc::kill(-stamp.pid, libc::SIGTERM);
    }
    let deadline = Instant::now() + Duration::from_secs(5);
    while group_alive(stamp.pid) && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(50));
    }
    if group_alive(stamp.pid) {
        unsafe {
            libc::kill(-stamp.pid, libc::SIGKILL);
        }
    }
    Ok(())
}
