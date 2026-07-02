use crate::{settings::Settings, system};
use std::process::Command;

pub fn capture_jpeg(settings: &Settings) -> Result<Vec<u8>, String> {
    let command = system::first_available_command(&["rpicam-still", "libcamera-still"])
        .ok_or_else(|| "No rpicam-still/libcamera-still command found.".to_string())?;

    let mut args = vec![
        "-o".to_string(),
        "-".to_string(),
        "--width".to_string(),
        settings.resolution_width.to_string(),
        "--height".to_string(),
        settings.resolution_height.to_string(),
        "--timeout".to_string(),
        "350".to_string(),
        "--nopreview".to_string(),
    ];

    if settings.hflip || settings.rotation == 180 {
        args.push("--hflip".to_string());
    }
    if settings.vflip || settings.rotation == 180 {
        args.push("--vflip".to_string());
    }
    if settings.rotation != 0 && settings.rotation != 180 {
        args.push("--rotation".to_string());
        args.push(settings.rotation.to_string());
    }

    let output = crate::proc::run(Command::new(&command).args(args), crate::proc::CAPTURE_TIMEOUT)
        .map_err(|error| error.to_string())?;
    if output.status.success() && !output.stdout.is_empty() {
        Ok(output.stdout)
    } else {
        let message = String::from_utf8_lossy(if output.stderr.is_empty() {
            &output.stdout
        } else {
            &output.stderr
        });
        Err(message.trim().to_string())
    }
}
