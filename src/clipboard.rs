use std::error::Error;
use std::process::{Command, Stdio};

pub trait Clipboard {
    fn copy(&self, text: &str) -> Result<(), Box<dyn Error>>;
    fn paste(&self) -> Result<String, Box<dyn Error>>;
}

pub struct WaylandClipboard;

impl Clipboard for WaylandClipboard {
    fn copy(&self, text: &str) -> Result<(), Box<dyn Error>> {
        let mut child = Command::new("wl-copy")
            .arg(text)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| format!("Failed to spawn wl-copy: {}", e))?;

        let status = child.wait()
            .map_err(|e| format!("Failed to wait for wl-copy: {}", e))?;

        if status.success() {
            Ok(())
        } else {
            let stderr = child.stderr.take().map_or_else(
                || "(No stderr)".to_string(),
                |e| std::io::read_to_string(e).unwrap_or_else(|_| "(Failed to read stderr)".to_string())
            );
            Err(format!("wl-copy failed with status: {:?}, stderr: {}", status, stderr).into())
        }
    }

    fn paste(&self) -> Result<String, Box<dyn Error>> {
        let child = Command::new("wl-paste")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| format!("Failed to spawn wl-paste: {}", e))?;

        let output = child.wait_with_output()
            .map_err(|e| format!("Failed to wait for wl-paste: {}", e))?;

        if output.status.success() {
            String::from_utf8(output.stdout)
                .map_err(|e| format!("Failed to decode wl-paste output: {}", e).into())
        } else {
            let stderr = String::from_utf8_lossy(&output.stderr);
            Err(format!("wl-paste failed with status: {:?}, stderr: {}", output.status, stderr).into())
        }
    }
}

// A no-op clipboard for environments where no system clipboard is available or supported.
#[allow(dead_code)]
pub struct NoopClipboard;

impl Clipboard for NoopClipboard {
    fn copy(&self, _text: &str) -> Result<(), Box<dyn Error>> {
        Err("No system clipboard available or supported.".into())
    }

    fn paste(&self) -> Result<String, Box<dyn Error>> {
        Err("No system clipboard available or supported.".into())
    }
}
