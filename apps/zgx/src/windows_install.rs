use std::path::PathBuf;

use zeron_update::windows::{self, Completion};

use crate::about::APP_VERSION;

/// An older updater may have installed only the executable. Complete its
/// version's folder before opening the UI or forwarding commands to the engine.
pub fn complete_if_needed(argv: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    windows::set_packaged();
    let command = argv.get(1).map(String::as_str);
    if matches!(
        command,
        Some("--version" | "-V" | "--help" | "-h" | "help" | "--noop-browser")
    ) {
        return Ok(());
    }
    let Some(directory) = windows::incomplete_install(&std::env::current_exe()?, APP_VERSION)
    else {
        return Ok(());
    };
    let opens_window =
        command.is_none_or(|arg| arg.starts_with("--") || arg.starts_with("zerun-dev://"));
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    loop {
        match runtime.block_on(windows::complete_install(
            zeron_cli::DEFAULT_EDGE_URL,
            &directory,
            APP_VERSION,
        )) {
            Ok(Completion::Complete | Completion::Installed) => return Ok(()),
            Ok(Completion::Updated(_)) => {
                let status = std::process::Command::new(directory.join("zerun.exe"))
                    .args(&argv[1..])
                    .status()?;
                std::process::exit(status.code().unwrap_or(1));
            }
            Err(error) => {
                let message = format!("Zerun couldn't finish installing its update.\n\n{error:#}");
                if !opens_window || !retry(&message) {
                    return Err(message.into());
                }
            }
        }
    }
}

pub fn version_files() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let directory = windows::install_directory(&exe)?;
    Some(windows::version_files(&directory, APP_VERSION))
}

fn retry(message: &str) -> bool {
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        IDRETRY, MB_ICONERROR, MB_RETRYCANCEL, MB_SETFOREGROUND, MessageBoxW,
    };
    let wide = |s: &str| {
        s.encode_utf16()
            .chain(std::iter::once(0))
            .collect::<Vec<u16>>()
    };
    let (text, caption) = (wide(message), wide("Zerun"));
    // SAFETY: both buffers are NUL-terminated and live for the call.
    let answer = unsafe {
        MessageBoxW(
            std::ptr::null_mut(),
            text.as_ptr(),
            caption.as_ptr(),
            MB_RETRYCANCEL | MB_ICONERROR | MB_SETFOREGROUND,
        )
    };
    answer == IDRETRY
}
