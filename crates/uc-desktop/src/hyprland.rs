//! Bounded Hyprland IPC for desktop focus and panel placement.

use std::io::{Read, Write};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use serde::Deserialize;

const IO_TIMEOUT: Duration = Duration::from_millis(150);
const MAX_RESPONSE: u64 = 1024 * 1024;

#[derive(Clone)]
pub struct Hyprland {
    socket: PathBuf,
}

// Deliberately not Debug: compositor metadata can contain user content.
#[derive(Clone, Deserialize, PartialEq, Eq)]
pub struct WindowTarget {
    address: String,
    pid: u32,
    #[serde(default)]
    class: String,
}

#[derive(Clone, Copy, Deserialize)]
pub struct CursorPosition {
    pub x: f64,
    pub y: f64,
}

impl Hyprland {
    pub fn current() -> Option<Self> {
        let runtime = std::env::var_os("XDG_RUNTIME_DIR")?;
        let instance = std::env::var("HYPRLAND_INSTANCE_SIGNATURE").ok()?;
        if instance.is_empty()
            || !instance
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
        {
            return None;
        }
        Some(Self {
            socket: PathBuf::from(runtime)
                .join("hypr")
                .join(instance)
                .join(".socket.sock"),
        })
    }

    fn request(&self, command: &str) -> Result<String, String> {
        let address = socket2::SockAddr::unix(&self.socket)
            .map_err(|_| "Invalid Hyprland IPC socket address")?;
        let socket = socket2::Socket::new(socket2::Domain::UNIX, socket2::Type::STREAM, None)
            .map_err(|_| "Cannot create Hyprland IPC socket")?;
        socket
            .connect_timeout(&address, IO_TIMEOUT)
            .map_err(|_| "Cannot connect to Hyprland IPC within the deadline")?;
        let mut socket = UnixStream::from(socket);
        socket
            .set_read_timeout(Some(IO_TIMEOUT))
            .map_err(|_| "Cannot bound Hyprland read")?;
        socket
            .set_write_timeout(Some(IO_TIMEOUT))
            .map_err(|_| "Cannot bound Hyprland write")?;
        socket
            .write_all(command.as_bytes())
            .map_err(|_| "Hyprland IPC write failed")?;
        let deadline = Instant::now() + IO_TIMEOUT;
        let mut response = Vec::new();
        let mut chunk = [0u8; 8192];
        loop {
            let remaining = deadline
                .checked_duration_since(Instant::now())
                .filter(|remaining| !remaining.is_zero())
                .ok_or("Hyprland IPC response deadline exceeded")?;
            socket
                .set_read_timeout(Some(remaining))
                .map_err(|_| "Cannot bound Hyprland read")?;
            let count = socket
                .read(&mut chunk)
                .map_err(|_| "Hyprland IPC read failed or timed out")?;
            if count == 0 {
                break;
            }
            if response.len() as u64 + count as u64 > MAX_RESPONSE {
                return Err("Hyprland IPC response exceeded the limit".into());
            }
            response.extend_from_slice(&chunk[..count]);
        }
        String::from_utf8(response).map_err(|_| "Invalid Hyprland response encoding".into())
    }

    /// Read the compositor's effective logical corner radius, including runtime overrides.
    pub fn window_corner_radius(&self) -> Result<u32, String> {
        #[derive(Deserialize)]
        struct Rounding {
            int: u32,
        }
        let value: Rounding =
            serde_json::from_str(&self.request("j/getoption decoration:rounding")?)
                .map_err(|_| "Invalid Hyprland rounding response")?;
        Ok(value.int)
    }

    pub fn cursor(&self) -> Result<CursorPosition, String> {
        let cursor: CursorPosition = serde_json::from_str(&self.request("j/cursorpos")?)
            .map_err(|_| "Invalid Hyprland cursor response")?;
        if !cursor.x.is_finite() || !cursor.y.is_finite() {
            return Err("Invalid Hyprland cursor coordinates".into());
        }
        Ok(cursor)
    }

    pub fn active_window(&self) -> Result<Option<WindowTarget>, String> {
        let response = self.request("j/activewindow")?;
        let value: serde_json::Value = serde_json::from_str(&response)
            .map_err(|_| "Invalid Hyprland active-window response")?;
        if value.as_object().is_some_and(|object| object.is_empty()) {
            return Ok(None);
        }
        let target: WindowTarget =
            serde_json::from_value(value).map_err(|_| "Incomplete Hyprland window identity")?;
        target.validate()?;
        Ok(Some(target))
    }

    fn dispatch(&self, lua: String) -> Result<(), String> {
        let response = self.request(&format!("/dispatch {lua}"))?;
        if response.trim() != "ok" {
            // Do not echo compositor responses; they can include window metadata.
            return Err("Hyprland rejected the panel input operation".into());
        }
        Ok(())
    }

    /// Call on a worker thread after the panel has released its keyboard focus.
    #[tracing::instrument(name = "desktop.panel.paste", skip_all)]
    pub fn paste(&self, target: &WindowTarget) -> Result<(), String> {
        let result = self.paste_inner(target);
        match &result {
            Ok(()) => tracing::info!("Panel paste shortcut delivered"),
            Err(error) => {
                tracing::warn!(error_kind = "panel_paste_failed", retryable = true, %error, "Panel paste failed")
            }
        }
        result
    }

    fn paste_inner(&self, target: &WindowTarget) -> Result<(), String> {
        target.validate()?;
        let clients: Vec<WindowTarget> = serde_json::from_str(&self.request("j/clients")?)
            .map_err(|_| "Invalid Hyprland client list")?;
        if !clients.iter().any(|client| client == target) {
            return Err("The previous application window is no longer available".into());
        }
        self.dispatch(format!(
            "hl.dsp.focus({{ window = \"address:{}\" }})",
            target.address
        ))?;
        let deadline = Instant::now() + Duration::from_millis(500);
        loop {
            if self.active_window()?.as_ref() == Some(target) {
                break;
            }
            if Instant::now() >= deadline {
                return Err("Previous application focus could not be confirmed".into());
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        self.dispatch(format!(
            "hl.dsp.send_shortcut({{ mods = \"{}\", key = \"V\", window = \"address:{}\" }})",
            paste_modifiers(&target.class),
            target.address
        ))
    }
}

impl WindowTarget {
    fn validate(&self) -> Result<(), String> {
        let hex = self.address.strip_prefix("0x").unwrap_or_default();
        if self.pid == 0
            || hex.is_empty()
            || hex.len() > 16
            || !hex.bytes().all(|b| b.is_ascii_hexdigit())
            || hex.bytes().all(|b| b == b'0')
        {
            return Err("Invalid Hyprland window identity".into());
        }
        Ok(())
    }
}

fn paste_modifiers(class: &str) -> &'static str {
    match class.to_ascii_lowercase().as_str() {
        "alacritty"
        | "kitty"
        | "foot"
        | "footclient"
        | "com.mitchellh.ghostty"
        | "org.wezfurlong.wezterm"
        | "org.gnome.terminal"
        | "konsole" => "CTRL SHIFT",
        _ => "CTRL",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_addresses_that_could_escape_the_dispatcher_argument() {
        for address in [
            "",
            "0x0",
            "0x",
            "0x123\"}",
            "0x123;exec",
            "0x12345678901234567",
        ] {
            assert!(WindowTarget {
                address: address.into(),
                pid: 42,
                class: String::new()
            }
            .validate()
            .is_err());
        }
        assert!(WindowTarget {
            address: "0xabcd1234".into(),
            pid: 42,
            class: String::new()
        }
        .validate()
        .is_ok());
    }

    #[test]
    fn terminal_paste_uses_shift_without_changing_regular_applications() {
        assert_eq!(paste_modifiers("Alacritty"), "CTRL SHIFT");
        assert_eq!(paste_modifiers("com.mitchellh.ghostty"), "CTRL SHIFT");
        assert_eq!(paste_modifiers("chromium"), "CTRL");
    }

    fn scripted_compositor(
        responses: Vec<&'static str>,
    ) -> (Hyprland, std::thread::JoinHandle<Vec<String>>) {
        use std::os::unix::net::UnixListener;
        let path = std::env::temp_dir().join(format!("uc-hypr-test-{}.sock", uuid::Uuid::new_v4()));
        let listener = UnixListener::bind(&path).expect("test listener");
        let socket_path = path.clone();
        let worker = std::thread::spawn(move || {
            let mut commands = Vec::new();
            for response in responses {
                let (mut socket, _) = listener.accept().expect("accept");
                let mut buffer = [0; 4096];
                let count = socket.read(&mut buffer).expect("read command");
                commands.push(String::from_utf8(buffer[..count].to_vec()).expect("command UTF-8"));
                socket
                    .write_all(response.as_bytes())
                    .expect("write response");
            }
            std::fs::remove_file(socket_path).expect("remove socket");
            commands
        });
        (Hyprland { socket: path }, worker)
    }

    #[test]
    fn reads_effective_rounding_and_rejects_invalid_values() {
        let (client, worker) = scripted_compositor(vec![
            r#"{"int":12}"#,
            r#"{"int":0}"#,
            r#"{"int":-1}"#,
            r#"{"int":1.5}"#,
            "{}",
        ]);
        assert_eq!(client.window_corner_radius().expect("rounding"), 12);
        assert_eq!(client.window_corner_radius().expect("square"), 0);
        for _ in 0..3 {
            assert!(client.window_corner_radius().is_err());
        }
        assert_eq!(
            worker.join().expect("worker"),
            vec!["j/getoption decoration:rounding"; 5]
        );
    }

    #[test]
    fn does_not_focus_or_paste_into_a_reused_window_address() {
        let (client, worker) =
            scripted_compositor(vec![r#"[{"address":"0x123","pid":99,"class":"test"}]"#]);
        let target = WindowTarget {
            address: "0x123".into(),
            pid: 42,
            class: "test".into(),
        };
        assert!(client.paste(&target).is_err());
        assert_eq!(worker.join().expect("worker"), vec!["j/clients"]);
    }

    #[test]
    fn sends_paste_only_after_the_target_identity_is_confirmed() {
        let (client, worker) = scripted_compositor(vec![
            r#"[{"address":"0x123","pid":42,"class":"kitty"}]"#,
            "ok",
            r#"{"address":"0x123","pid":42,"class":"kitty"}"#,
            "ok",
        ]);
        let target = WindowTarget {
            address: "0x123".into(),
            pid: 42,
            class: "kitty".into(),
        };
        client.paste(&target).expect("paste");
        let commands = worker.join().expect("worker");
        assert_eq!(commands[0], "j/clients");
        assert!(commands[1].contains("hl.dsp.focus"));
        assert_eq!(commands[2], "j/activewindow");
        assert!(commands[3].contains("send_shortcut"));
        assert!(commands[3].contains("CTRL SHIFT"));
        assert!(commands[3].contains("address:0x123"));
    }

    #[test]
    fn bounds_a_compositor_that_never_replies() {
        use std::os::unix::net::UnixListener;
        let path = std::env::temp_dir().join(format!("uc-hypr-test-{}.sock", uuid::Uuid::new_v4()));
        let listener = UnixListener::bind(&path).expect("test socket");
        let worker = std::thread::spawn(move || {
            let (_stream, _) = listener.accept().expect("accept");
            std::thread::sleep(Duration::from_millis(400));
        });
        let client = Hyprland {
            socket: path.clone(),
        };
        let start = Instant::now();
        assert!(client.cursor().is_err());
        assert!(start.elapsed() < Duration::from_secs(1));
        worker.join().expect("worker");
        std::fs::remove_file(path).expect("remove socket");
    }
}
