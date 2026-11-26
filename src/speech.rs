#[cfg(any(target_os = "macos", target_os = "windows"))]
use std::io::Write;
use std::process::Command;
#[cfg(any(target_os = "macos", target_os = "windows"))]
use std::process::Stdio;

/// Reads `text` aloud with the system voice. The process is started here so errors can be
/// reported; writing the text and waiting for it to finish happen on a background
/// thread, so long pages don't freeze the window.
pub fn speak(text: &str) -> Result<(), String> {
    let text = text.trim();
    if text.is_empty() {
        return Ok(());
    }
    #[cfg(target_os = "linux")]
    {
        // `--` keeps text that starts with "-" from being read as an option.
        let mut child = Command::new("spd-say")
            .arg("--")
            .arg(text)
            .spawn()
            .map_err(|error| {
                crate::i18n::t!("error-speech-dispatcher", error = error.to_string()).into_owned()
            })?;
        std::thread::spawn(move || {
            let _ = child.wait();
        });
        Ok(())
    }
    #[cfg(target_os = "macos")]
    {
        // With no message arguments, `say` reads the text from standard input.
        let child = Command::new("say")
            .stdin(Stdio::piped())
            .spawn()
            .map_err(|error| {
                crate::i18n::t!("error-system-voice", error = error.to_string()).into_owned()
            })?;
        feed_and_wait(child, text.to_string());
        Ok(())
    }
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        let child = Command::new("powershell")
            .args([
                "-NoProfile",
                "-NonInteractive",
                "-Command",
                "[Console]::InputEncoding = [System.Text.Encoding]::UTF8; Add-Type -AssemblyName System.Speech; $text=[Console]::In.ReadToEnd(); (New-Object System.Speech.Synthesis.SpeechSynthesizer).Speak($text)",
            ])
            .stdin(Stdio::piped())
            .creation_flags(CREATE_NO_WINDOW)
            .spawn()
            .map_err(|error| {
                crate::i18n::t!("error-system-voice", error = error.to_string()).into_owned()
            })?;
        feed_and_wait(child, text.to_string());
        Ok(())
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
    {
        let _ = Command::new;
        Err(crate::i18n::t!("error-read-aloud-unsupported").into_owned())
    }
}

#[cfg(any(target_os = "macos", target_os = "windows"))]
fn feed_and_wait(mut child: std::process::Child, text: String) {
    std::thread::spawn(move || {
        if let Some(mut stdin) = child.stdin.take() {
            let _ = stdin.write_all(text.as_bytes());
        }
        let _ = child.wait();
    });
}
