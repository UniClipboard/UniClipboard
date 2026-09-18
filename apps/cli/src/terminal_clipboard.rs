use std::io::{self, IsTerminal, Write};

use base64::engine::general_purpose::STANDARD;
use base64::Engine as _;

pub fn copy(value: &str) -> Result<(), String> {
    let stderr = io::stderr();
    if !stderr.is_terminal() {
        return Err(
            "Cannot copy because the terminal output is not connected to a terminal".to_string(),
        );
    }

    let mut terminal = stderr.lock();
    write_osc52(&mut terminal, value)
        .map_err(|err| format!("Failed to send the result to the terminal clipboard: {err}"))?;
    terminal
        .flush()
        .map_err(|err| format!("Failed to flush the terminal clipboard request: {err}"))
}

fn write_osc52(writer: &mut impl Write, value: &str) -> io::Result<()> {
    let request = format!("\x1b]52;c;{}\x07", STANDARD.encode(value.as_bytes()));
    writer.write_all(request.as_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn writes_osc52_sequence_for_unicode_text() {
        let mut output = Vec::new();

        write_osc52(&mut output, "hello, 世界").expect("OSC 52 write should succeed");

        assert_eq!(output, b"\x1b]52;c;aGVsbG8sIOS4lueVjA==\x07");
    }

    #[test]
    fn writes_osc52_sequence_for_empty_text() {
        let mut output = Vec::new();

        write_osc52(&mut output, "").expect("OSC 52 write should succeed");

        assert_eq!(output, b"\x1b]52;c;\x07");
    }
}
