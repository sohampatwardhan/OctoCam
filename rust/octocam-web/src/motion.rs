use crate::settings;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;
use tokio::io::AsyncReadExt;
use tokio::sync::broadcast;

pub fn spawn_motion_detector(
    config_path: std::path::PathBuf,
    motion_detected: Arc<AtomicBool>,
    motion_tx: broadcast::Sender<bool>,
) {
    tokio::spawn(async move {
        let mut previous_state = false;

        loop {
            // Load settings
            let settings = settings::load_settings(&config_path);

            if !settings.motion_enabled {
                if previous_state {
                    motion_detected.store(false, Ordering::Relaxed);
                    let _ = motion_tx.send(false);
                    previous_state = false;
                }
                tokio::time::sleep(Duration::from_secs(2)).await;
                continue;
            }

            // Motion is enabled, let's start the capture loop
            let path = if settings.sub_stream_enabled {
                &settings.sub_rtsp_path
            } else {
                &settings.rtsp_path
            };
            let url = format!("rtsp://127.0.0.1:8554/{}", path.trim_start_matches('/'));

            tracing::info!("Starting motion detection loop against RTSP source: {}", url);

            let mut child = match tokio::process::Command::new("ffmpeg")
                .args([
                    "-hide_banner",
                    "-loglevel",
                    "error",
                    "-rtsp_transport",
                    "tcp",
                    "-i",
                    &url,
                    "-vf",
                    "scale=80:60,fps=5,format=gray",
                    "-f",
                    "rawvideo",
                    "-",
                ])
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::null())
                .spawn()
            {
                Ok(c) => c,
                Err(e) => {
                    tracing::error!("Failed to spawn ffmpeg for motion detection: {e}");
                    tokio::time::sleep(Duration::from_secs(5)).await;
                    continue;
                }
            };

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
                        break; // exit ffmpeg loop
                    }
                }

                // Read next frame (4800 bytes)
                match stdout.read_exact(&mut current_frame).await {
                    Ok(_) => {
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
                                let _ = motion_tx.send(current_state);
                                previous_state = current_state;
                            }
                        }

                        previous_frame.copy_from_slice(&current_frame);
                        has_previous = true;
                    }
                    Err(e) => {
                        tracing::error!("Error reading from ffmpeg stream stdout: {e}");
                        break; // exit ffmpeg loop to restart
                    }
                }
            }

            // Clean up child process
            let _ = child.kill().await;
            tracing::info!("Motion detection loop exited. Restarting in 5 seconds...");
            tokio::time::sleep(Duration::from_secs(5)).await;
        }
    });
}
