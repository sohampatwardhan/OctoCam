use std::io::{self, Read};
use std::process::{Command, ExitStatus, Output, Stdio};
use std::thread;
use std::time::{Duration, Instant};

/// Quick local queries (ip, iw link, systemctl show, hostname, wpa_cli, command -v).
pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(5);
/// Wi-Fi scans and camera enumeration, which are legitimately slow.
pub const SCAN_TIMEOUT: Duration = Duration::from_secs(12);
/// systemctl enable/disable/start/stop/restart.
pub const SERVICE_TIMEOUT: Duration = Duration::from_secs(10);
/// nmcli connect / connection up (association + DHCP).
pub const CONNECT_TIMEOUT: Duration = Duration::from_secs(25);
/// rpicam-still / libcamera-still capture.
pub const CAPTURE_TIMEOUT: Duration = Duration::from_secs(8);

/// How often the wait loop polls the child while waiting for exit or timeout.
const POLL_INTERVAL: Duration = Duration::from_millis(20);

/// Run `command`, capturing stdout/stderr, but never block longer than `timeout`.
///
/// Behavior:
/// - stdin is `/dev/null` so a child can never block waiting for input.
/// - stdout and stderr are drained on dedicated threads, so a child that writes
///   more than the pipe buffer (~64 KiB) cannot deadlock against a full pipe.
/// - On timeout the child is killed and an `io::Error` of kind `TimedOut` is returned.
///
/// Returns the same `io::Result<Output>` shape as `Command::output()`, so a
/// non-zero exit is `Ok(output)` with `output.status.success() == false`.
///
/// NOTE: `child.kill()` targets only the direct child (matching Tokio's own
/// examples). Every current call site runs a single binary (`nmcli`, `iw`,
/// `wpa_cli`, `systemctl`, `sh -c "command -v ..."`, `rpicam-*`) that does not
/// background a surviving grandchild, so killing the child reliably closes the
/// pipes and the reader threads see EOF. Do NOT pass a shell string that
/// backgrounds a process (`&`, `nohup`, `setsid`) without switching to a
/// process-group kill first, or the reader-thread join below could block.
pub fn run(command: &mut Command, timeout: Duration) -> io::Result<Output> {
    command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    let mut child = command.spawn()?;

    let mut child_stdout = child.stdout.take().expect("stdout piped");
    let mut child_stderr = child.stderr.take().expect("stderr piped");
    let out_handle = thread::spawn(move || {
        let mut buf = Vec::new();
        let _ = child_stdout.read_to_end(&mut buf);
        buf
    });
    let err_handle = thread::spawn(move || {
        let mut buf = Vec::new();
        let _ = child_stderr.read_to_end(&mut buf);
        buf
    });

    let start = Instant::now();
    let status: ExitStatus = loop {
        if let Some(status) = child.try_wait()? {
            break status;
        }
        if start.elapsed() >= timeout {
            let _ = child.kill();
            let _ = child.wait();
            // Child is dead; readers hit EOF. Join so no threads/pipes leak.
            let _ = out_handle.join();
            let _ = err_handle.join();
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                format!("command timed out after {timeout:?}"),
            ));
        }
        thread::sleep(POLL_INTERVAL);
    };

    let stdout = out_handle.join().unwrap_or_default();
    let stderr = err_handle.join().unwrap_or_default();
    Ok(Output {
        status,
        stdout,
        stderr,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn returns_stdout_for_fast_command() {
        let out = run(Command::new("sh").args(["-c", "printf hello"]), DEFAULT_TIMEOUT)
            .expect("command should run");
        assert!(out.status.success());
        assert_eq!(out.stdout, b"hello");
    }

    #[test]
    fn nonzero_exit_is_ok_not_err() {
        let out = run(Command::new("sh").args(["-c", "exit 3"]), DEFAULT_TIMEOUT)
            .expect("command should run");
        assert!(!out.status.success());
    }

    #[test]
    fn kills_and_errors_on_timeout() {
        let start = Instant::now();
        let err = run(Command::new("sh").args(["-c", "sleep 30"]), Duration::from_millis(300))
            .expect_err("command should time out");
        assert_eq!(err.kind(), io::ErrorKind::TimedOut);
        // Must return promptly, not after the full 30s sleep.
        assert!(start.elapsed() < Duration::from_secs(5));
    }

    #[test]
    fn captures_large_output_without_deadlock() {
        // ~220 KB, well past the typical 64 KiB pipe buffer.
        let out = run(
            Command::new("sh").args(["-c", "yes 0123456789 | head -n 20000"]),
            Duration::from_secs(10),
        )
        .expect("command should run");
        assert!(out.status.success());
        assert!(out.stdout.len() > 100_000);
    }

    #[test]
    fn spawn_failure_is_err() {
        let err = run(
            &mut Command::new("definitely-not-a-real-binary-xyz"),
            DEFAULT_TIMEOUT,
        )
        .expect_err("spawn should fail");
        assert_eq!(err.kind(), io::ErrorKind::NotFound);
    }
}
