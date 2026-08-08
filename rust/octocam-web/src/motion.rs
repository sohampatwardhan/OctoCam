use crate::settings;
use serde::Serialize;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, BufReader};
use tokio::sync::broadcast;

/// How stale the last decoded frame may be before the detector is reported as
/// not covering the scene.
///
/// Sized above the ~8.5s RTSP startup (see [`FIRST_FRAME_TIMEOUT`]) plus a
/// retry, so a routine reconnect doesn't flap the signal — a health indicator
/// that cries wolf gets ignored, which would defeat its purpose. Still far
/// below the interval over which a missed intruder matters.
pub const MOTION_STALE_AFTER: Duration = Duration::from_secs(30);

/// Startup and steady state are different regimes and need different budgets.
///
/// The *first* frame costs an RTSP DESCRIBE/SETUP/PLAY handshake, a wait for the
/// next keyframe, and decoder/swscale init. Measured on the Pi Zero 2 W against
/// the local sub stream: 7.0s, 8.9s, 7.3s. This budget is deliberately several
/// times that worst case — overshooting merely delays detecting a genuinely dead
/// source, while undershooting kills every session before it yields a frame and
/// takes motion detection down completely.
const FIRST_FRAME_TIMEOUT: Duration = Duration::from_secs(30);

/// Once frames are flowing, a healthy 5 fps source delivers one every 200 ms, so
/// silence this long means the pipeline is wedged: tear it down and reconnect
/// rather than blocking forever.
///
/// This also bounds how long a wedged stream can ignore a settings change:
/// the reload check below only runs between frame reads.
///
/// NOTE: `read_exact` is *not* cancel-safe — a timed-out read may have already
/// consumed part of a frame, which would desync every subsequent frame. So a
/// timeout must always restart ffmpeg; it can never simply retry the read.
const FRAME_READ_TIMEOUT: Duration = Duration::from_secs(2);

/// Reconnect delays, in order, for consecutive failed sessions. Every second
/// spent here is a second of missed motion, so the first few retries are
/// deliberately aggressive: respawning ffmpeg is cheap, and a transient RTSP
/// drop usually clears immediately. The tail exists only to avoid a spawn storm
/// when the camera is genuinely down — a case where retrying fast wouldn't have
/// caught anything anyway.
const RECONNECT_BACKOFF: [Duration; 5] = [
    Duration::from_millis(250),
    Duration::from_millis(500),
    Duration::from_secs(2),
    Duration::from_secs(5),
    Duration::from_secs(10),
];

/// A session that ran at least this long is treated as healthy: the next
/// failure starts backoff over from the fast end of the ladder.
const SESSION_HEALTHY_AFTER: Duration = Duration::from_secs(60);

/// Liveness of the motion detector, shared with the HTTP layer.
///
/// Exists because `motion_detected: false` is ambiguous: it means both "nothing
/// is moving" and "the detector is dead and cannot see anything". For a sensor
/// whose whole job is not missing events, silent blindness is the worst failure
/// mode, so availability is tracked and published explicitly.
#[derive(Debug)]
pub struct MotionHealth {
    /// Unix millis of the last successfully decoded frame; 0 if never.
    last_frame_ms: AtomicU64,
    /// Consecutive failed sessions; 0 while healthy.
    consecutive_failures: AtomicU32,
    /// Cumulative milliseconds spent not covering the scene since boot.
    total_blind_ms: AtomicU64,
    /// Whether motion detection is switched on in settings.
    enabled: AtomicBool,
    /// Whether a session is currently producing frames.
    streaming: AtomicBool,
}

/// Point-in-time view of [`MotionHealth`], safe to serialize to API clients.
#[derive(Debug, Clone, Serialize)]
pub struct MotionHealthView {
    /// The headline signal: is the detector actually covering the scene right
    /// now? Drives HomeKit's `StatusActive`.
    pub available: bool,
    /// Finer-grained reason, for UI copy that distinguishes "off" from "broken".
    /// One of `ok`, `starting`, `reconnecting`, `down`, `disabled`.
    pub state: &'static str,
    /// Age of the newest decoded frame, or `None` if no frame has ever arrived.
    pub last_frame_age_ms: Option<u64>,
    pub consecutive_failures: u32,
    pub total_blind_ms: u64,
}

impl Default for MotionHealth {
    fn default() -> Self {
        Self {
            last_frame_ms: AtomicU64::new(0),
            consecutive_failures: AtomicU32::new(0),
            total_blind_ms: AtomicU64::new(0),
            enabled: AtomicBool::new(false),
            streaming: AtomicBool::new(false),
        }
    }
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

impl MotionHealth {
    fn mark_frame(&self) {
        self.last_frame_ms.store(now_ms(), Ordering::Relaxed);
        self.streaming.store(true, Ordering::Relaxed);
        self.consecutive_failures.store(0, Ordering::Relaxed);
    }

    fn set_enabled(&self, enabled: bool) {
        self.enabled.store(enabled, Ordering::Relaxed);
        if !enabled {
            self.streaming.store(false, Ordering::Relaxed);
        }
    }

    fn mark_session_ended(&self, failures: u32, blind: Duration) {
        self.streaming.store(false, Ordering::Relaxed);
        self.consecutive_failures.store(failures, Ordering::Relaxed);
        self.total_blind_ms
            .fetch_add(blind.as_millis() as u64, Ordering::Relaxed);
    }

    pub fn snapshot(&self) -> MotionHealthView {
        let last = self.last_frame_ms.load(Ordering::Relaxed);
        let age_ms = (last != 0).then(|| now_ms().saturating_sub(last));
        let enabled = self.enabled.load(Ordering::Relaxed);
        let streaming = self.streaming.load(Ordering::Relaxed);
        let fresh = age_ms.is_some_and(|age| age < MOTION_STALE_AFTER.as_millis() as u64);

        let (available, state) = match (enabled, streaming, fresh, age_ms.is_some()) {
            (false, ..) => (false, "disabled"),
            (true, true, true, _) => (true, "ok"),
            // Frames were flowing moments ago; a reconnect is in flight. Not
            // covering the scene, but not a fault worth alarming on yet.
            (true, _, true, _) => (false, "reconnecting"),
            // Enabled but no frame has ever arrived: still coming up.
            (true, _, false, false) => (false, "starting"),
            (true, ..) => (false, "down"),
        };

        MotionHealthView {
            available,
            state,
            last_frame_age_ms: age_ms,
            consecutive_failures: self.consecutive_failures.load(Ordering::Relaxed),
            total_blind_ms: self.total_blind_ms.load(Ordering::Relaxed),
        }
    }
}

/// Pushed to SSE subscribers whenever detection state or availability changes.
///
/// Availability rides the same channel as detection so consumers (the HomeKit
/// bridge, the dashboard) learn about blindness the moment it happens rather
/// than inferring it from a poll gap.
#[derive(Debug, Clone, Copy, Serialize)]
pub struct MotionUpdate {
    pub motion_detected: bool,
    pub motion_available: bool,
}

/// Why a capture session ended, for logging and backoff decisions.
enum SessionEnd {
    /// Motion detection was switched off — a clean stop, not a failure.
    Disabled,
    /// ffmpeg's stdout closed; the usual `early eof`.
    StreamClosed(std::io::Error),
    /// The stream never produced a first frame within [`FIRST_FRAME_TIMEOUT`].
    StartupTimeout,
    /// Frames were flowing, then stopped for [`FRAME_READ_TIMEOUT`].
    Stalled,
}

impl std::fmt::Display for SessionEnd {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Disabled => write!(f, "motion detection disabled"),
            Self::StreamClosed(e) => write!(f, "ffmpeg stdout closed: {e}"),
            Self::StartupTimeout => write!(
                f,
                "no first frame within {}s of connecting",
                FIRST_FRAME_TIMEOUT.as_secs()
            ),
            Self::Stalled => write!(
                f,
                "frames stopped for {}s",
                FRAME_READ_TIMEOUT.as_secs()
            ),
        }
    }
}

pub fn spawn_motion_detector(
    config_path: std::path::PathBuf,
    motion_detected: Arc<AtomicBool>,
    motion_tx: broadcast::Sender<MotionUpdate>,
    health: Arc<MotionHealth>,
) {
    tokio::spawn(async move {
        // Publish detection state and availability together, so a subscriber can
        // never see one without the other.
        let publish = {
            let health = health.clone();
            let motion_tx = motion_tx.clone();
            move |detected: bool| {
                let _ = motion_tx.send(MotionUpdate {
                    motion_detected: detected,
                    motion_available: health.snapshot().available,
                });
            }
        };

        let mut previous_state = false;
        let mut previous_available = false;
        // Session bookkeeping, so restarts are attributable and their cost in
        // missed coverage is measurable rather than inferred from the journal.
        let mut session_id: u64 = 0;
        let mut consecutive_failures: usize = 0;
        let mut blind_time = Duration::ZERO;

        loop {
            // Load settings
            let settings = settings::load_settings(&config_path);

            if !settings.motion_enabled {
                health.set_enabled(false);
                if previous_state || previous_available {
                    motion_detected.store(false, Ordering::Relaxed);
                    previous_state = false;
                    previous_available = false;
                    publish(false);
                }
                tokio::time::sleep(Duration::from_secs(2)).await;
                continue;
            }
            health.set_enabled(true);

            // Always read the main path, never sub — even when sub is enabled.
            //
            // `sub` is a derived, on-demand stream: mediamtx spawns a software
            // x264 transcoder (runOnDemand) to produce it, and that transcoder
            // measured at ~2.4 cores on the Pi Zero 2 W. Because motion is
            // otherwise the only 24/7 reader of `sub`, reading it here forced
            // that transcoder to run continuously — pushing load average past
            // the core count, which in turn starved the hardware encoder
            // (`VIDIOC_QBUF` failures) and tore `sub` down, the actual cause of
            // the frequent `early eof` reconnects.
            //
            // `main` is the always-on hardware-encoded source, so reading it
            // removes both the fragile second hop and the transcoder's CPU cost
            // (it now only runs when a human is actually viewing sub). Decoding
            // the larger main frame costs this detector's own ffmpeg more, but
            // that is a fraction of the ~2.4 cores freed. The reader reserve in
            // mediamtx.rs already budgets motion's slot on every path.
            let path = &settings.rtsp_path;
            let url = format!("rtsp://127.0.0.1:8554/{}", path.trim_start_matches('/'));

            session_id += 1;
            tracing::info!("Motion session {session_id} starting against RTSP source: {url}");

            let mut child = match tokio::process::Command::new("ffmpeg")
                .args([
                    "-hide_banner",
                    // `error` hid the cause of the frequent `early eof` restarts;
                    // RTSP teardowns and timeouts are reported at warning level.
                    "-loglevel",
                    "warning",
                    // Cheap-decode flags: this detector downscales to 80x60 gray,
                    // so decode fidelity is irrelevant. Skipping the deblocking
                    // loop filter cuts the cost of decoding main's full 1296x972
                    // substantially. Deliberately NOT skipping frames (e.g.
                    // -skip_frame nonref): the 5fps filter needs a steady frame
                    // supply, and starving it would undercut availability.
                    "-flags2",
                    "+fast",
                    "-skip_loop_filter",
                    "all",
                    "-rtsp_transport",
                    "tcp",
                    "-i",
                    &url,
                    // Drop to 5fps BEFORE scaling so the scaler only touches the
                    // frames we keep — matters more now that the source is main's
                    // full 1296x972 rather than sub's 640x480. (Decode still runs
                    // on every input frame; only the scale work is saved.)
                    "-vf",
                    "fps=5,scale=80:60,format=gray",
                    "-f",
                    "rawvideo",
                    "-",
                ])
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::piped())
                // If this task is ever dropped mid-session, don't strand ffmpeg
                // holding one of the sub-stream's reader slots.
                .kill_on_drop(true)
                .spawn()
            {
                Ok(c) => c,
                Err(e) => {
                    tracing::error!("Failed to spawn ffmpeg for motion detection: {e}");
                    consecutive_failures += 1;
                    let backoff = reconnect_delay(consecutive_failures);
                    blind_time += backoff;
                    tokio::time::sleep(backoff).await;
                    continue;
                }
            };

            // ffmpeg's diagnostics used to go to /dev/null, which is why the
            // restarts had no attributable cause. Tag each line with the session
            // so a restart can be correlated with what ffmpeg said beforehand.
            if let Some(stderr) = child.stderr.take() {
                tokio::spawn(async move {
                    let mut lines = BufReader::new(stderr).lines();
                    while let Ok(Some(line)) = lines.next_line().await {
                        tracing::warn!("motion ffmpeg[{session_id}]: {line}");
                    }
                });
            }

            let session_start = std::time::Instant::now();
            let mut frames_read: u64 = 0;
            // Frame accounting starts when frames start, so the expected-frame
            // figure isn't skewed by the ~8s RTSP startup.
            let mut first_frame_at: Option<std::time::Instant> = None;
            let session_end;

            let mut stdout = child.stdout.take().unwrap();
            let mut current_frame = vec![0u8; 4800];
            let mut previous_frame = vec![0u8; 4800];
            let mut has_previous = false;
            let mut consecutive_motion_frames = 0;
            let mut cooldown_remaining = 0;

            let mut last_settings_reload = std::time::Instant::now();
            let mut current_settings = settings.clone();

            loop {
                // Periodically reload settings (every 2 seconds) to pick up config updates
                if last_settings_reload.elapsed() > Duration::from_secs(2) {
                    current_settings = settings::load_settings(&config_path);
                    last_settings_reload = std::time::Instant::now();

                    if !current_settings.motion_enabled {
                        session_end = SessionEnd::Disabled;
                        break; // exit ffmpeg loop
                    }
                }

                // Read next frame (4800 bytes). A timeout here is terminal: see
                // the cancel-safety note on FRAME_READ_TIMEOUT.
                let (budget, timeout_reason) = if frames_read == 0 {
                    (FIRST_FRAME_TIMEOUT, SessionEnd::StartupTimeout)
                } else {
                    (FRAME_READ_TIMEOUT, SessionEnd::Stalled)
                };

                let read = match tokio::time::timeout(
                    budget,
                    stdout.read_exact(&mut current_frame),
                )
                .await
                {
                    Ok(result) => result,
                    Err(_) => {
                        session_end = timeout_reason;
                        break;
                    }
                };

                match read {
                    Ok(_) => {
                        health.mark_frame();
                        if frames_read == 0 {
                            let startup = session_start.elapsed();
                            first_frame_at = Some(std::time::Instant::now());
                            tracing::info!(
                                "Motion session {session_id} streaming; first frame after {:.1}s",
                                startup.as_secs_f64(),
                            );
                        }
                        frames_read += 1;

                        // Coverage resumed — tell subscribers immediately rather
                        // than leaving HomeKit showing an inactive sensor until
                        // the next detection transition.
                        if !previous_available {
                            previous_available = true;
                            publish(previous_state);
                        }
                        if has_previous {
                            let mut changed_count = 0;
                            let mut active_pixels = 0;
                            let mut global_changed_count = 0;

                            for idx in 0..4800 {
                                let x = idx % 80;
                                let y = idx / 80;

                                // Map 80x60 to 8x8 grid:
                                // col: x / 10 (0..7)
                                // row: y / 7.5 (0..7)
                                let col = x / 10;
                                let row = (y as f64 / 7.5) as usize;
                                let grid_idx = row.min(7) * 8 + col.min(7);

                                let active = (current_settings.motion_zones & (1u64 << grid_idx)) != 0;

                                let diff = (current_frame[idx] as i16 - previous_frame[idx] as i16).abs();
                                if diff > 25 {
                                    global_changed_count += 1;
                                    if active {
                                        changed_count += 1;
                                    }
                                }
                                if active {
                                    active_pixels += 1;
                                }
                            }

                            let motion_detected_this_frame = if active_pixels > 0 {
                                let changed_percentage = (changed_count as f64 / active_pixels as f64) * 100.0;
                                // Map sensitivity (1..100) to a threshold percentage.
                                // Higher sensitivity = lower threshold.
                                let threshold_pct = (101 - current_settings.motion_sensitivity) as f64 * 0.05;

                                let is_local_motion = changed_percentage >= threshold_pct;
                                let is_global_change = (global_changed_count as f64 / 4800.0) * 100.0 > 75.0;

                                is_local_motion && !is_global_change
                            } else {
                                false
                            };

                            let mut current_state = previous_state;
                            if motion_detected_this_frame {
                                consecutive_motion_frames += 1;
                                if consecutive_motion_frames >= 2 {
                                    current_state = true;
                                    cooldown_remaining = 25; // 5 seconds at 5 FPS
                                }
                            } else {
                                consecutive_motion_frames = 0;
                                if cooldown_remaining > 0 {
                                    cooldown_remaining -= 1;
                                    if cooldown_remaining == 0 {
                                        current_state = false;
                                    }
                                } else {
                                    current_state = false;
                                }
                            }

                            if current_state != previous_state {
                                tracing::info!("Motion detection state changed: {current_state}");
                                motion_detected.store(current_state, Ordering::Relaxed);
                                previous_state = current_state;
                                publish(current_state);
                            }
                        }

                        previous_frame.copy_from_slice(&current_frame);
                        has_previous = true;
                    }
                    Err(e) => {
                        session_end = SessionEnd::StreamClosed(e);
                        break; // exit ffmpeg loop to restart
                    }
                }
            }

            // Clean up child process
            let _ = child.kill().await;

            let elapsed = session_start.elapsed();
            let expected_frames = first_frame_at
                .map(|t| t.elapsed().as_secs_f64() * 5.0 + 1.0)
                .unwrap_or(0.0);

            if matches!(session_end, SessionEnd::Disabled) {
                tracing::info!(
                    "Motion session {session_id} stopped after {:.1}s ({frames_read} frames): {session_end}",
                    elapsed.as_secs_f64(),
                );
                consecutive_failures = 0;
                health.mark_session_ended(0, Duration::ZERO);
                continue;
            }

            // A long-lived session that just died is a fresh fault, not an
            // ongoing outage — retry from the fast end of the ladder.
            if elapsed >= SESSION_HEALTHY_AFTER {
                consecutive_failures = 0;
            }
            consecutive_failures += 1;
            let backoff = reconnect_delay(consecutive_failures);
            // The reconnect also costs the ~8.5s RTSP startup, so the true gap
            // is larger than the backoff; the frame-age signal reflects that
            // even though this counter only accrues the deliberate wait.
            blind_time += backoff;
            health.mark_session_ended(consecutive_failures as u32, backoff);

            // Availability just dropped. Publish before sleeping so consumers
            // see the outage at its start, not after the reconnect completes.
            if previous_available {
                previous_available = false;
                publish(previous_state);
            }

            tracing::warn!(
                "Motion session {session_id} ended after {:.1}s: {session_end}. \
                 Read {frames_read} frames (expected ~{expected_frames:.0}); \
                 failure #{consecutive_failures}, reconnecting in {:.0}ms. \
                 Cumulative blind time since boot: {:.1}s",
                elapsed.as_secs_f64(),
                backoff.as_secs_f64() * 1000.0,
                blind_time.as_secs_f64(),
            );

            tokio::time::sleep(backoff).await;
        }
    });
}

/// Delay before reconnecting after `failures` consecutive failed sessions.
/// Saturates at the last rung rather than growing without bound.
fn reconnect_delay(failures: usize) -> Duration {
    let idx = failures.saturating_sub(1).min(RECONNECT_BACKOFF.len() - 1);
    RECONNECT_BACKOFF[idx]
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Backdate the last frame so staleness can be exercised without sleeping.
    fn with_frame_age(health: &MotionHealth, age: Duration) {
        health.last_frame_ms.store(
            now_ms().saturating_sub(age.as_millis() as u64),
            Ordering::Relaxed,
        );
    }

    #[test]
    fn disabled_reports_unavailable_but_not_a_fault() {
        let health = MotionHealth::default();
        health.mark_frame();
        health.set_enabled(false);

        let view = health.snapshot();
        // Switched off is not covering the scene, so `available` must be false —
        // but the UI needs to tell this apart from a broken detector.
        assert!(!view.available);
        assert_eq!(view.state, "disabled");
    }

    #[test]
    fn streaming_with_a_fresh_frame_is_ok() {
        let health = MotionHealth::default();
        health.set_enabled(true);
        health.mark_frame();

        let view = health.snapshot();
        assert!(view.available);
        assert_eq!(view.state, "ok");
    }

    #[test]
    fn enabled_with_no_frame_yet_is_starting_not_down() {
        let health = MotionHealth::default();
        health.set_enabled(true);

        let view = health.snapshot();
        // ~8.5s of RTSP startup is normal; flagging it as a fault would make the
        // signal flap on every restart and train users to ignore it.
        assert!(!view.available);
        assert_eq!(view.state, "starting");
        assert_eq!(view.last_frame_age_ms, None);
    }

    #[test]
    fn session_end_with_a_recent_frame_is_reconnecting() {
        let health = MotionHealth::default();
        health.set_enabled(true);
        health.mark_frame();
        health.mark_session_ended(1, Duration::from_millis(250));

        let view = health.snapshot();
        assert!(!view.available);
        assert_eq!(view.state, "reconnecting");
        assert_eq!(view.consecutive_failures, 1);
    }

    #[test]
    fn a_stale_frame_is_down_even_if_streaming_was_never_cleared() {
        let health = MotionHealth::default();
        health.set_enabled(true);
        health.mark_frame();
        with_frame_age(&health, MOTION_STALE_AFTER + Duration::from_secs(5));

        let view = health.snapshot();
        // Guards the wedged-pipeline case: `streaming` stays true because no
        // code path ran to clear it, so freshness has to be the deciding input.
        assert!(!view.available);
        assert_eq!(view.state, "down");
    }

    #[test]
    fn recovery_clears_failures_and_restores_availability() {
        let health = MotionHealth::default();
        health.set_enabled(true);
        health.mark_session_ended(4, Duration::from_secs(10));
        assert_eq!(health.snapshot().state, "starting");

        health.mark_frame();

        let view = health.snapshot();
        assert!(view.available);
        assert_eq!(view.state, "ok");
        assert_eq!(view.consecutive_failures, 0);
        // Blind time is cumulative and must survive recovery.
        assert_eq!(view.total_blind_ms, 10_000);
    }

    #[test]
    fn reconnect_delay_starts_fast_and_saturates() {
        // Coverage matters more than politeness on the first retries.
        assert_eq!(reconnect_delay(1), Duration::from_millis(250));
        assert_eq!(reconnect_delay(2), Duration::from_millis(500));
        // Saturates rather than growing without bound.
        assert_eq!(reconnect_delay(99), *RECONNECT_BACKOFF.last().unwrap());
    }
}
