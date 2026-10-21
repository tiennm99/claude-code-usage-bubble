//! OS-specific code. Each platform file is compiled only on its own target,
//! and portable callers reach it through `cfg`-gated calls.

use std::time::{Duration, Instant};

#[cfg(target_os = "linux")]
pub mod linux;
#[cfg(target_os = "macos")]
pub mod macos;
#[cfg(windows)]
pub mod windows;

/// How often the Unix wait checks whether the process is still there.
#[cfg(unix)]
const WAIT_POLL_INTERVAL: Duration = Duration::from_millis(100);

/// Wait until process `pid` has exited, at most `timeout`. Returns true when
/// it is gone, false when it was still running at the deadline.
pub fn wait_pid(pid: u32, timeout: Duration) -> bool {
    #[cfg(windows)]
    {
        windows::wait_pid(pid, timeout)
    }
    #[cfg(unix)]
    {
        wait_until_gone(timeout, WAIT_POLL_INTERVAL, || unix_pid_alive(pid))
    }
}

/// Poll `alive` every `interval` until it returns false or `timeout` passes.
/// Returns true when `alive` reported false.
pub fn wait_until_gone(
    timeout: Duration,
    interval: Duration,
    mut alive: impl FnMut() -> bool,
) -> bool {
    let deadline = Instant::now() + timeout;
    loop {
        if !alive() {
            return true;
        }
        let now = Instant::now();
        if now >= deadline {
            return false;
        }
        std::thread::sleep(interval.min(deadline - now));
    }
}

/// Whether `pid` names a live (or zombie) process. A process owned by
/// another user still counts as alive.
#[cfg(unix)]
fn unix_pid_alive(pid: u32) -> bool {
    // Zero and negative ids address process groups, never one process.
    let Ok(pid) = libc::pid_t::try_from(pid) else {
        return false;
    };
    if pid <= 0 {
        return false;
    }
    // SAFETY: signal 0 only checks for existence and permission.
    if unsafe { libc::kill(pid, 0) } == 0 {
        return true;
    }
    std::io::Error::last_os_error().raw_os_error() == Some(libc::EPERM)
}

/// Whether the system theme is dark at startup. Platforms without a
/// readable system setting default to dark.
pub fn system_is_dark() -> bool {
    #[cfg(windows)]
    {
        windows::is_dark()
    }
    #[cfg(not(windows))]
    {
        true
    }
}

/// Whether the app can place its windows. False only under a native
/// Wayland GTK backend (no Xwayland), where moves are ignored and positions
/// read back as `(0, 0)`, so snapping, clamping and saving are skipped.
pub fn positioning_supported() -> bool {
    #[cfg(target_os = "linux")]
    {
        linux::positioning_supported(
            std::env::var_os("WAYLAND_DISPLAY"),
            std::env::var_os("GDK_BACKEND"),
        )
    }
    #[cfg(not(target_os = "linux"))]
    {
        true
    }
}

/// Whether the primary mouse button is held right now, where the platform
/// can tell; false otherwise.
pub fn primary_button_down() -> bool {
    #[cfg(target_os = "linux")]
    {
        linux::button1_down()
    }
    #[cfg(target_os = "macos")]
    {
        macos::primary_button_down()
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wait_until_gone_returns_once_the_process_is_gone() {
        let mut checks = 0;
        let gone = wait_until_gone(Duration::from_secs(5), Duration::from_millis(1), || {
            checks += 1;
            checks < 3
        });
        assert!(gone);
        assert_eq!(checks, 3);
    }

    #[test]
    fn wait_until_gone_gives_up_at_the_deadline() {
        let start = Instant::now();
        let gone = wait_until_gone(Duration::from_millis(30), Duration::from_millis(5), || true);
        assert!(!gone);
        assert!(start.elapsed() >= Duration::from_millis(30));
        assert!(start.elapsed() < Duration::from_secs(2));
    }

    #[test]
    fn a_running_process_is_waited_for_until_the_timeout() {
        assert!(!wait_pid(std::process::id(), Duration::from_millis(50)));
    }

    #[cfg(unix)]
    #[test]
    fn an_exited_process_is_gone() {
        let mut child = std::process::Command::new("true")
            .spawn()
            .expect("spawn true");
        let pid = child.id();
        // Reap it, so the pid no longer names even a zombie.
        child.wait().expect("wait");
        assert!(wait_pid(pid, Duration::from_secs(2)));
    }

    #[cfg(unix)]
    #[test]
    fn group_ids_are_never_alive() {
        assert!(!unix_pid_alive(0));
        assert!(!unix_pid_alive(u32::MAX));
    }
}
