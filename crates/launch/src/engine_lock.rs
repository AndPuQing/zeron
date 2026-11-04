//! Probe the engine's data-directory lock without linking the engine.

use std::fs::OpenOptions;
use std::path::Path;

/// The PID stamped by a running engine, or `None` when its data-directory
/// lock is free. This single non-blocking probe never retries or takes over
/// an active engine's lock; service installation can safely check ownership.
pub fn engine_lock_holder(data_dir: &Path) -> Option<String> {
    let path = data_dir.join("engine.lock");
    #[cfg(unix)]
    {
        use std::os::unix::io::AsRawFd;
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(&path)
            .ok()?;
        let rc = unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) };
        if rc == 0 {
            // We took it: nothing is running. Closing the fd releases it, but
            // unlock explicitly so the window is as small as possible.
            unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_UN) };
            return None;
        }
        let pid = std::fs::read_to_string(&path).unwrap_or_default();
        let pid = pid.trim();
        Some(if pid.is_empty() {
            "unknown".to_string()
        } else {
            pid.to_string()
        })
    }
    #[cfg(windows)]
    {
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(&path)
            .ok()?;
        match file.try_lock() {
            Ok(()) => {
                let _ = file.unlock();
                None
            }
            Err(std::fs::TryLockError::WouldBlock) => Some(windows_holder_pid(data_dir)),
            Err(std::fs::TryLockError::Error(_)) => None,
        }
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = path;
        None
    }
}

#[cfg(windows)]
fn windows_holder_pid(data_dir: &Path) -> String {
    std::fs::read_to_string(data_dir.join("engine.lock.pid"))
        .unwrap_or_default()
        .trim()
        .to_string()
}
