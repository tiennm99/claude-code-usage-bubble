// Reach into installed WSL distros to read their Claude credentials.
//
// We never mount the WSL filesystem ourselves — instead we shell out to
// `wsl.exe -d <distro> -- sh -lc '...'` and read stdout. Every call has
// a hard timeout so a hung WSL doesn't freeze the poll thread.

use std::os::windows::process::CommandExt;
use std::process::{Command, Output, Stdio};
use std::time::{Duration, Instant};

use super::local_fs::parse_claude_json;

const CREATE_NO_WINDOW: u32 = 0x0800_0000;
const COMMAND_TIMEOUT: Duration = Duration::from_secs(5);

/// Enumerate installed WSL distributions. Returns an empty vec if WSL is
/// not installed or the probe fails.
pub fn list_distros() -> Vec<String> {
    let Some(output) = run_with_timeout(
        Command::new("wsl.exe").args(["-l", "-q"]),
        COMMAND_TIMEOUT,
    ) else {
        return Vec::new();
    };
    if !output.status.success() {
        return Vec::new();
    }
    decode_wsl_text(&output.stdout)
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .map(ToOwned::to_owned)
        .collect()
}

pub struct WslClaudeCreds {
    distro: String,
    id: String,
}

impl WslClaudeCreds {
    pub fn new(distro: String) -> Self {
        let id = format!("wsl:{distro}");
        Self { distro, id }
    }

    pub fn distro(&self) -> &str {
        &self.distro
    }
}

impl super::CredentialSource for WslClaudeCreds {
    fn id(&self) -> &str {
        &self.id
    }

    fn read(&self) -> Result<super::Token, super::Error> {
        let output = wsl_run(&self.distro, "cat ~/.claude/.credentials.json")?;
        if !output.status.success() {
            return Err(super::Error::WslCommand {
                distro: self.distro.clone(),
                detail: format!("cat exited {}", output.status),
            });
        }
        let content = String::from_utf8(output.stdout).map_err(|_| super::Error::WslCommand {
            distro: self.distro.clone(),
            detail: "non-UTF-8 stdout".into(),
        })?;
        parse_claude_json(&content)
    }

    fn signature(&self) -> Option<String> {
        let output = wsl_run(
            &self.distro,
            "if [ -f ~/.claude/.credentials.json ]; then \
             stat -c '%s|%Y' ~/.claude/.credentials.json; \
             else echo MISSING; fi",
        )
        .ok()?;
        if !output.status.success() {
            return None;
        }
        let body = decode_wsl_text(&output.stdout).trim().to_string();
        if body == "MISSING" {
            return None;
        }
        Some(format!("{}|{}", self.id, body))
    }

    fn refresh_hint(&self) -> super::RefreshHint {
        super::RefreshHint::WslClaudeCli {
            distro: self.distro.clone(),
        }
    }
}

fn wsl_run(distro: &str, script: &str) -> Result<Output, super::Error> {
    run_with_timeout(
        Command::new("wsl.exe")
            .arg("-d")
            .arg(distro)
            .arg("--")
            .arg("sh")
            .arg("-lc")
            .arg(script),
        COMMAND_TIMEOUT,
    )
    .ok_or(super::Error::WslTimeout)
}

fn run_with_timeout(cmd: &mut Command, timeout: Duration) -> Option<Output> {
    let mut child = cmd
        .creation_flags(CREATE_NO_WINDOW)
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    let start = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(_)) => return child.wait_with_output().ok(),
            Ok(None) => {
                if start.elapsed() > timeout {
                    let _ = child.kill();
                    let _ = child.wait();
                    return None;
                }
                std::thread::sleep(Duration::from_millis(80));
            }
            Err(_) => return None,
        }
    }
}

/// `wsl.exe -l -q` historically emits UTF-16LE on stdout; other commands
/// emit UTF-8. Detect by sampling high bytes and decode appropriately.
pub(crate) fn decode_wsl_text(bytes: &[u8]) -> String {
    if bytes.len() >= 2 && bytes.len() % 2 == 0 {
        let sample_end = bytes.len().min(256);
        let mut high_nul = 0usize;
        for chunk in bytes[..sample_end].chunks_exact(2) {
            if chunk[1] == 0 {
                high_nul += 1;
            }
        }
        if high_nul * 2 >= sample_end / 2 {
            let units: Vec<u16> = bytes
                .chunks_exact(2)
                .map(|c| u16::from_le_bytes([c[0], c[1]]))
                .collect();
            return String::from_utf16_lossy(&units);
        }
    }
    String::from_utf8_lossy(bytes).into_owned()
}
