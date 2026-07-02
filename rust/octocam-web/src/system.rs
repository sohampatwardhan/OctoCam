use serde::Serialize;
use std::{collections::HashMap, fs, path::Path, process::Command, thread, time::Duration};

#[derive(Clone, Debug, Serialize)]
pub struct SystemStatus {
    pub hostname: String,
    pub ip_addresses: Vec<String>,
    pub uptime: Option<String>,
    pub cpu_temp_c: Option<f64>,
    pub resources: ResourceStatus,
    pub wifi: WifiStatus,
    pub camera: CameraStatus,
    pub services: Services,
    pub logs: Vec<String>,
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct ResourceStatus {
    pub cpu_usage_percent: Option<f64>,
    pub load_average: Option<String>,
    pub memory: MemoryStatus,
    pub memory_summary: Option<String>,
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct MemoryStatus {
    pub total_mb: i64,
    pub available_mb: i64,
    pub used_mb: i64,
    pub used_percent: Option<f64>,
    pub swap_total_mb: i64,
    pub swap_used_mb: i64,
    pub swap_used_percent: Option<f64>,
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct WifiStatus {
    pub ssid: Option<String>,
    pub state: String,
    pub message: String,
    pub source: Option<String>,
    pub interface: Option<String>,
    pub bssid: Option<String>,
    pub frequency_mhz: Option<i32>,
    pub channel: Option<i32>,
    pub band: Option<String>,
    pub channel_width: Option<String>,
    pub signal_dbm: Option<String>,
    pub rx_bitrate: Option<String>,
    pub tx_bitrate: Option<String>,
    pub tx_power: Option<String>,
    pub wifi_generation: Option<String>,
    pub wifi_generation_label: Option<String>,
    pub security: Option<String>,
    pub pairwise_cipher: Option<String>,
    pub group_cipher: Option<String>,
    pub ip_address: Option<String>,
    pub ip_addresses: Vec<String>,
    pub mac_address: Option<String>,
    pub default_gateway: Option<String>,
    pub default_interface: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct CameraStatus {
    pub available: bool,
    pub tool: Option<String>,
    pub message: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct ServiceStatus {
    pub unit: String,
    pub state: String,
    pub enabled: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct Services {
    pub octocam_web: ServiceStatus,
    pub homekit: ServiceStatus,
    pub rtsp: ServiceStatus,
}

#[derive(Clone, Debug)]
pub struct LabelValue {
    pub label: String,
    pub value: String,
}

#[derive(Clone, Debug)]
pub struct StoredWifiProfile {
    pub name: String,
    pub security: String,
    pub source: String,
    pub active: bool,
    pub can_delete: bool,
    pub delete_source: String,
}

#[derive(Clone, Debug)]
pub struct SystemView {
    pub camera_available: bool,
    pub camera_label: String,
    pub wifi_label: String,
    pub wifi_signal_percent: f64,
    pub wifi_signal_level: String,
    pub wifi_signal_label: String,
    pub wifi_ip_addresses: String,
    pub ip_addresses: String,
    pub uptime: String,
    pub cpu_temp: String,
    pub cpu_usage: String,
    pub cpu_usage_percent: f64,
    pub load_average: String,
    pub memory: String,
    pub memory_percent: f64,
    pub has_swap: bool,
    pub swap: String,
    pub swap_percent: f64,
    pub web_state: String,
    pub rtsp_state: String,
    pub homekit_state: String,
    pub wifi_details: Vec<LabelValue>,
    pub logs: Vec<String>,
    pub ssh_target: String,
}

pub fn status() -> SystemStatus {
    SystemStatus {
        hostname: hostname(),
        ip_addresses: ip_addresses(),
        uptime: uptime_text(),
        cpu_temp_c: cpu_temp_c(),
        resources: resource_status(),
        wifi: wifi_status(),
        camera: camera_status(),
        services: Services {
            octocam_web: service_status("octocam-web"),
            homekit: service_status("octocam-homekit"),
            rtsp: service_status("octocam-rtsp"),
        },
        logs: service_logs("octocam-web", 40),
    }
}

pub fn view(status: &SystemStatus) -> SystemView {
    let ip_addresses = if status.ip_addresses.is_empty() {
        "Not available".to_string()
    } else {
        status.ip_addresses.join(", ")
    };
    let ssh_target = status
        .ip_addresses
        .first()
        .cloned()
        .unwrap_or_else(|| format!("{}.local", status.hostname));
    let memory = status
        .resources
        .memory_summary
        .clone()
        .unwrap_or_else(|| "Not available".to_string());
    let swap = format!(
        "{} / {} MB{}",
        status.resources.memory.swap_used_mb,
        status.resources.memory.swap_total_mb,
        status
            .resources
            .memory
            .swap_used_percent
            .map(|value| format!(" ({value:.1}%)"))
            .unwrap_or_default()
    );
    let wifi_signal_percent = wifi_signal_percent(&status.wifi);
    let wifi_signal_level = wifi_signal_level(wifi_signal_percent).to_string();
    let wifi_signal_label = status
        .wifi
        .signal_dbm
        .as_ref()
        .map(|value| format!("Signal {value} ({wifi_signal_percent:.0}%)"))
        .unwrap_or_else(|| "Signal unavailable".to_string());
    let wifi_ip_addresses = wifi_ip_addresses_text(&status.wifi);

    SystemView {
        camera_available: status.camera.available,
        camera_label: if status.camera.available {
            "Camera online"
        } else {
            "Camera unavailable"
        }
        .to_string(),
        wifi_label: status
            .wifi
            .ssid
            .clone()
            .unwrap_or_else(|| status.wifi.message.clone()),
        wifi_signal_percent,
        wifi_signal_level,
        wifi_signal_label,
        wifi_ip_addresses,
        ip_addresses,
        uptime: status
            .uptime
            .clone()
            .unwrap_or_else(|| "Not available".to_string()),
        cpu_temp: status
            .cpu_temp_c
            .map(|value| format!("{value:.1} C"))
            .unwrap_or_else(|| "Not available".to_string()),
        cpu_usage: status
            .resources
            .cpu_usage_percent
            .map(|value| format!("{value:.1}%"))
            .unwrap_or_else(|| "Not available".to_string()),
        cpu_usage_percent: clamp_percent(status.resources.cpu_usage_percent.unwrap_or_default()),
        load_average: status
            .resources
            .load_average
            .clone()
            .unwrap_or_else(|| "Not available".to_string()),
        memory,
        memory_percent: clamp_percent(status.resources.memory.used_percent.unwrap_or_default()),
        has_swap: status.resources.memory.swap_total_mb > 0,
        swap,
        swap_percent: clamp_percent(
            status
                .resources
                .memory
                .swap_used_percent
                .unwrap_or_default(),
        ),
        web_state: status.services.octocam_web.state.clone(),
        rtsp_state: status.services.rtsp.state.clone(),
        homekit_state: status.services.homekit.state.clone(),
        wifi_details: wifi_details(&status.wifi),
        logs: status.logs.clone(),
        ssh_target,
    }
}

pub fn stored_wifi_profiles(active_wifi: &WifiStatus) -> Vec<StoredWifiProfile> {
    let active_ssid = active_wifi.ssid.as_deref();
    let mut profiles = Vec::new();
    profiles.extend(network_manager_profiles(active_ssid));
    profiles.extend(wpa_supplicant_profiles(active_ssid));
    profiles.extend(dietpi_autosetup_profiles(active_ssid));

    let mut seen = Vec::new();
    profiles.retain(|profile| {
        let key = format!("{}:{}", profile.source, profile.name);
        if seen.iter().any(|existing| existing == &key) {
            false
        } else {
            seen.push(key);
            true
        }
    });
    profiles
}

fn clamp_percent(value: f64) -> f64 {
    value.clamp(0.0, 100.0)
}

fn wifi_signal_percent(wifi: &WifiStatus) -> f64 {
    let Some(dbm) = wifi_signal_dbm(wifi) else {
        return 0.0;
    };
    clamp_percent(((dbm + 100.0) / 50.0) * 100.0)
}

fn wifi_signal_level(percent: f64) -> &'static str {
    if percent >= 67.0 {
        "high"
    } else if percent >= 34.0 {
        "low"
    } else {
        "zero"
    }
}

fn wifi_signal_dbm(wifi: &WifiStatus) -> Option<f64> {
    wifi.signal_dbm
        .as_deref()?
        .split_whitespace()
        .next()?
        .parse()
        .ok()
}

fn wifi_ip_addresses_text(wifi: &WifiStatus) -> String {
    if !wifi.ip_addresses.is_empty() {
        wifi.ip_addresses.join(", ")
    } else if let Some(address) = &wifi.ip_address {
        address.clone()
    } else {
        "Not available".to_string()
    }
}

pub fn set_service_enabled(unit: &str, enabled: bool) -> Result<(), String> {
    if !command_exists("systemctl") {
        return Err("systemctl not found".to_string());
    }
    let action = if enabled { "enable" } else { "disable" };
    let state_action = if enabled { "start" } else { "stop" };
    for args in [[action, unit], [state_action, unit]] {
        let output = Command::new("systemctl")
            .args(args)
            .output()
            .map_err(|error| error.to_string())?;
        if !output.status.success() {
            let message = String::from_utf8_lossy(if output.stderr.is_empty() {
                &output.stdout
            } else {
                &output.stderr
            });
            return Err(message.trim().to_string());
        }
    }
    Ok(())
}

pub fn restart_service(unit: &str) -> Result<(), String> {
    if !command_exists("systemctl") {
        return Err("systemctl not found".to_string());
    }
    let output = Command::new("systemctl")
        .args(["restart", unit])
        .output()
        .map_err(|error| error.to_string())?;
    if !output.status.success() {
        let message = String::from_utf8_lossy(if output.stderr.is_empty() {
            &output.stdout
        } else {
            &output.stderr
        });
        return Err(message.trim().to_string());
    }
    Ok(())
}

pub fn command_exists(command: &str) -> bool {
    Command::new("sh")
        .args([
            "-c",
            &format!("command -v {} >/dev/null 2>&1", shell_escape(command)),
        ])
        .status()
        .map(|status| status.success())
        .unwrap_or(false)
}

pub fn first_available_command(names: &[&str]) -> Option<String> {
    names
        .iter()
        .find(|name| command_exists(name))
        .map(|name| (*name).to_string())
}

pub fn run_output(command: &str, args: &[&str]) -> Option<String> {
    let output = Command::new(command).args(args).output().ok()?;
    Some(
        String::from_utf8_lossy(if output.stdout.is_empty() {
            &output.stderr
        } else {
            &output.stdout
        })
        .trim()
        .to_string(),
    )
}

fn hostname() -> String {
    run_output("hostname", &[])
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "OctoCam".to_string())
}

fn ip_addresses() -> Vec<String> {
    run_output("hostname", &["-I"])
        .unwrap_or_default()
        .split_whitespace()
        .map(str::to_string)
        .collect()
}

fn cpu_temp_c() -> Option<f64> {
    let raw = fs::read_to_string("/sys/class/thermal/thermal_zone0/temp").ok()?;
    let value = raw.trim().parse::<f64>().ok()?;
    Some((value / 100.0).round() / 10.0)
}

fn resource_status() -> ResourceStatus {
    let memory = memory_status();
    let memory_summary = if memory.total_mb > 0 {
        Some(format!(
            "{} / {} MB ({:.1}%)",
            memory.used_mb,
            memory.total_mb,
            memory.used_percent.unwrap_or(0.0)
        ))
    } else {
        None
    };
    ResourceStatus {
        cpu_usage_percent: cpu_usage_percent(),
        load_average: load_average(),
        memory,
        memory_summary,
    }
}

fn cpu_usage_percent() -> Option<f64> {
    let first = read_cpu_times()?;
    thread::sleep(Duration::from_millis(120));
    let second = read_cpu_times()?;
    let idle_delta = second.0 - first.0;
    let total_delta = second.1 - first.1;
    if total_delta <= 0 {
        None
    } else {
        Some(((1.0 - idle_delta as f64 / total_delta as f64) * 1000.0).round() / 10.0)
    }
}

fn read_cpu_times() -> Option<(i64, i64)> {
    let raw = fs::read_to_string("/proc/stat").ok()?;
    let fields: Vec<i64> = raw
        .lines()
        .next()?
        .split_whitespace()
        .skip(1)
        .filter_map(|value| value.parse().ok())
        .collect();
    if fields.len() < 4 {
        return None;
    }
    let idle = fields[3] + fields.get(4).copied().unwrap_or(0);
    Some((idle, fields.iter().sum()))
}

fn load_average() -> Option<String> {
    let raw = fs::read_to_string("/proc/loadavg").ok()?;
    let values: Vec<&str> = raw.split_whitespace().take(3).collect();
    if values.len() == 3 {
        Some(values.join(", "))
    } else {
        None
    }
}

fn memory_status() -> MemoryStatus {
    let mut values = HashMap::<String, i64>::new();
    if let Ok(raw) = fs::read_to_string("/proc/meminfo") {
        for line in raw.lines() {
            let Some((key, rest)) = line.split_once(':') else {
                continue;
            };
            if let Some(value) = rest
                .split_whitespace()
                .next()
                .and_then(|value| value.parse::<i64>().ok())
            {
                values.insert(key.to_string(), value);
            }
        }
    }
    let total = values.get("MemTotal").copied().unwrap_or(0);
    let available = values.get("MemAvailable").copied().unwrap_or(0);
    let used = (total - available).max(0);
    let swap_total = values.get("SwapTotal").copied().unwrap_or(0);
    let swap_free = values.get("SwapFree").copied().unwrap_or(0);
    let swap_used = (swap_total - swap_free).max(0);
    MemoryStatus {
        total_mb: kib_to_mb(total),
        available_mb: kib_to_mb(available),
        used_mb: kib_to_mb(used),
        used_percent: if total > 0 {
            Some((used as f64 / total as f64 * 1000.0).round() / 10.0)
        } else {
            None
        },
        swap_total_mb: kib_to_mb(swap_total),
        swap_used_mb: kib_to_mb(swap_used),
        swap_used_percent: if swap_total > 0 {
            Some((swap_used as f64 / swap_total as f64 * 1000.0).round() / 10.0)
        } else {
            None
        },
    }
}

fn kib_to_mb(value: i64) -> i64 {
    ((value as f64) / 1024.0).round() as i64
}

fn uptime_text() -> Option<String> {
    let raw = fs::read_to_string("/proc/uptime").ok()?;
    let seconds = raw.split_whitespace().next()?.parse::<f64>().ok()? as i64;
    let days = seconds / 86_400;
    let hours = (seconds % 86_400) / 3600;
    let minutes = (seconds % 3600) / 60;
    if days > 0 {
        Some(format!("{days}d {hours}h"))
    } else if hours > 0 {
        Some(format!("{hours}h {minutes}m"))
    } else {
        Some(format!("{minutes}m"))
    }
}

fn wifi_status() -> WifiStatus {
    for detector in [
        nmcli_wifi_status as fn() -> Option<WifiStatus>,
        iw_wifi_status,
        wpa_cli_wifi_status,
    ] {
        if let Some(status) = detector() {
            if status.state == "connected" {
                return enrich_wifi_status(status);
            }
        }
    }
    if !["nmcli", "iw", "wpa_cli"]
        .iter()
        .any(|command| command_exists(command))
    {
        return WifiStatus {
            state: "unavailable".to_string(),
            message: "No Wi-Fi status tool found.".to_string(),
            ..Default::default()
        };
    }
    WifiStatus {
        state: "disconnected".to_string(),
        message: "No active Wi-Fi connection.".to_string(),
        ..Default::default()
    }
}

fn nmcli_wifi_status() -> Option<WifiStatus> {
    if !command_exists("nmcli") {
        return None;
    }
    let output = run_output("nmcli", &["-t", "-f", "ACTIVE,SSID", "dev", "wifi"])?;
    for line in output.lines() {
        let fields = split_nmcli_fields(line);
        if fields.len() >= 2 && fields[0] == "yes" {
            return Some(connected_wifi(Some(fields[1].clone()), "nmcli", None, None));
        }
    }
    None
}

fn iw_wifi_status() -> Option<WifiStatus> {
    if !command_exists("iw") {
        return None;
    }
    for interface in wireless_interfaces() {
        let output = run_output("iw", &["dev", &interface, "link"])?;
        let mut status: Option<WifiStatus> = None;
        for line in output.lines() {
            let stripped = line.trim();
            if let Some(rest) = stripped.strip_prefix("Connected to ") {
                let bssid = rest.split_whitespace().next().map(str::to_string);
                status = Some(connected_wifi(
                    None,
                    &format!("iw:{interface}"),
                    Some(interface.clone()),
                    bssid,
                ));
            } else if let Some(ssid) = stripped.strip_prefix("SSID:") {
                let mut next = status.unwrap_or_else(|| {
                    connected_wifi(
                        None,
                        &format!("iw:{interface}"),
                        Some(interface.clone()),
                        None,
                    )
                });
                next.ssid = Some(ssid.trim().to_string());
                return Some(next);
            }
        }
        if status.is_some() {
            return status;
        }
    }
    None
}

fn wpa_cli_wifi_status() -> Option<WifiStatus> {
    if !command_exists("wpa_cli") {
        return None;
    }
    for interface in wireless_interfaces()
        .into_iter()
        .map(Some)
        .chain(std::iter::once(None))
    {
        let mut args = Vec::new();
        if let Some(interface) = &interface {
            args.extend(["-i", interface.as_str()]);
        }
        args.push("status");
        let output = run_output("wpa_cli", &args)?;
        let fields = key_value_lines(&output);
        if fields
            .get("wpa_state")
            .map(|value| value == "COMPLETED")
            .unwrap_or(false)
        {
            if let Some(ssid) = fields.get("ssid") {
                return Some(connected_wifi(
                    Some(ssid.clone()),
                    "wpa_cli",
                    interface,
                    None,
                ));
            }
        }
    }
    None
}

fn connected_wifi(
    ssid: Option<String>,
    source: &str,
    interface: Option<String>,
    bssid: Option<String>,
) -> WifiStatus {
    WifiStatus {
        ssid,
        state: "connected".to_string(),
        message: "Connected".to_string(),
        source: Some(source.to_string()),
        interface,
        bssid,
        ..Default::default()
    }
}

fn enrich_wifi_status(mut status: WifiStatus) -> WifiStatus {
    if status.interface.is_none() {
        status.interface = wireless_interfaces().into_iter().next();
    }
    if let Some(interface) = status.interface.clone() {
        merge_wifi(&mut status, iw_link_details(&interface));
        merge_wifi(&mut status, wpa_status_details(&interface));
        merge_wifi(&mut status, iw_interface_details(&interface));
    }
    merge_wifi(&mut status, route_details());
    let address_interface = status
        .interface
        .clone()
        .or_else(|| status.default_interface.clone());
    if let Some(interface) = address_interface {
        let addresses = interface_ip_addresses(&interface);
        if !addresses.is_empty() {
            status.ip_addresses = addresses;
        }
    }
    if status.ip_addresses.is_empty() {
        if let Some(address) = &status.ip_address {
            status.ip_addresses.push(address.clone());
        }
    }
    if let Some(frequency) = status.frequency_mhz {
        status.channel = frequency_to_channel(frequency);
        status.band = Some(frequency_band(frequency));
    }
    if let Some(generation) = &status.wifi_generation {
        status.wifi_generation_label = Some(wifi_generation_label(generation));
    }
    status
}

fn merge_wifi(status: &mut WifiStatus, next: WifiStatus) {
    macro_rules! merge {
        ($field:ident) => {
            if next.$field.is_some() {
                status.$field = next.$field;
            }
        };
    }
    merge!(ssid);
    merge!(interface);
    merge!(bssid);
    merge!(frequency_mhz);
    merge!(channel);
    merge!(band);
    merge!(channel_width);
    merge!(signal_dbm);
    merge!(rx_bitrate);
    merge!(tx_bitrate);
    merge!(tx_power);
    merge!(wifi_generation);
    merge!(wifi_generation_label);
    merge!(security);
    merge!(pairwise_cipher);
    merge!(group_cipher);
    merge!(ip_address);
    merge!(mac_address);
    merge!(default_gateway);
    merge!(default_interface);
    if !next.ip_addresses.is_empty() {
        status.ip_addresses = next.ip_addresses;
    }
}

fn wireless_interfaces() -> Vec<String> {
    if command_exists("iw") {
        if let Some(output) = run_output("iw", &["dev"]) {
            let interfaces: Vec<String> = output
                .lines()
                .filter_map(|line| {
                    line.trim()
                        .strip_prefix("Interface ")
                        .map(|value| value.trim().to_string())
                })
                .collect();
            if !interfaces.is_empty() {
                return interfaces;
            }
        }
    }
    fs::read_dir("/sys/class/net")
        .ok()
        .into_iter()
        .flat_map(|entries| entries.flatten())
        .filter_map(|entry| entry.file_name().into_string().ok())
        .filter(|name| name.starts_with("wlan") || name.starts_with("wl"))
        .collect()
}

fn iw_link_details(interface: &str) -> WifiStatus {
    let Some(output) = run_output("iw", &["dev", interface, "link"]) else {
        return WifiStatus::default();
    };
    let mut details = WifiStatus::default();
    for line in output.lines() {
        let stripped = line.trim();
        if let Some(rest) = stripped.strip_prefix("Connected to ") {
            details.bssid = rest.split_whitespace().next().map(str::to_string);
            details.interface = Some(interface.to_string());
        } else if let Some(value) = stripped.strip_prefix("SSID:") {
            details.ssid = Some(value.trim().to_string());
        } else if let Some(value) = stripped.strip_prefix("freq:") {
            details.frequency_mhz = value.trim().parse().ok();
        } else if let Some(value) = stripped.strip_prefix("signal:") {
            details.signal_dbm = Some(value.trim().to_string());
        } else if let Some(value) = stripped.strip_prefix("rx bitrate:") {
            details.rx_bitrate = Some(value.trim().to_string());
        } else if let Some(value) = stripped.strip_prefix("tx bitrate:") {
            details.tx_bitrate = Some(value.trim().to_string());
        }
    }
    details
}

fn wpa_status_details(interface: &str) -> WifiStatus {
    let Some(output) = run_output("wpa_cli", &["-i", interface, "status"]) else {
        return WifiStatus::default();
    };
    let fields = key_value_lines(&output);
    WifiStatus {
        bssid: fields.get("bssid").cloned(),
        ssid: fields.get("ssid").cloned(),
        frequency_mhz: fields.get("freq").and_then(|value| value.parse().ok()),
        wifi_generation: fields.get("wifi_generation").cloned(),
        security: fields.get("key_mgmt").cloned(),
        pairwise_cipher: fields.get("pairwise_cipher").cloned(),
        group_cipher: fields.get("group_cipher").cloned(),
        ip_address: fields.get("ip_address").cloned(),
        mac_address: fields.get("address").cloned(),
        ..Default::default()
    }
}

fn iw_interface_details(interface: &str) -> WifiStatus {
    let Some(output) = run_output("iw", &["dev", interface, "info"]) else {
        return WifiStatus::default();
    };
    let mut details = WifiStatus::default();
    for line in output.lines() {
        let stripped = line.trim();
        if let Some(value) = stripped.strip_prefix("addr ") {
            details.mac_address = Some(value.trim().to_string());
        } else if let Some(value) = stripped.strip_prefix("channel ") {
            let parts: Vec<&str> = value.split_whitespace().collect();
            details.channel = parts.first().and_then(|value| value.parse().ok());
            details.frequency_mhz = parts
                .get(1)
                .and_then(|value| value.trim_matches(['(', ')']).parse().ok());
            if let Some(width) = stripped
                .split("width: ")
                .nth(1)
                .and_then(|value| value.split(',').next())
            {
                details.channel_width = Some(width.trim().to_string());
            }
        } else if let Some(value) = stripped.strip_prefix("txpower ") {
            details.tx_power = Some(value.trim().to_string());
        }
    }
    details
}

fn route_details() -> WifiStatus {
    let Some(output) = run_output("ip", &["route", "show", "default"]) else {
        return WifiStatus::default();
    };
    for line in output.lines() {
        let fields: Vec<&str> = line.split_whitespace().collect();
        if fields.first() != Some(&"default") {
            continue;
        }
        return WifiStatus {
            default_gateway: value_after(&fields, "via"),
            default_interface: value_after(&fields, "dev"),
            ..Default::default()
        };
    }
    WifiStatus::default()
}

fn interface_ip_addresses(interface: &str) -> Vec<String> {
    let Some(output) = run_output(
        "ip",
        &["-o", "addr", "show", "dev", interface, "scope", "global"],
    ) else {
        return Vec::new();
    };

    output
        .lines()
        .filter_map(|line| {
            let fields: Vec<&str> = line.split_whitespace().collect();
            match fields.get(2).copied() {
                Some("inet") | Some("inet6") => fields
                    .get(3)
                    .map(|address| address.split('/').next().unwrap_or(address).to_string()),
                _ => None,
            }
        })
        .collect()
}

fn network_manager_profiles(active_ssid: Option<&str>) -> Vec<StoredWifiProfile> {
    let Some(output) = run_output("nmcli", &["-t", "-f", "NAME,TYPE", "connection", "show"]) else {
        return Vec::new();
    };

    output
        .lines()
        .filter_map(|line| {
            let fields = split_nmcli_fields(line);
            if fields.len() < 2 || fields.get(1).map(String::as_str) != Some("802-11-wireless") {
                return None;
            }
            let name = fields[0].trim().to_string();
            if name.is_empty() || name == "OctoCam-Setup" {
                return None;
            }
            Some(StoredWifiProfile {
                active: active_ssid == Some(name.as_str()),
                name,
                security: "Saved".to_string(),
                source: "NetworkManager".to_string(),
                can_delete: true,
                delete_source: "network_manager".to_string(),
            })
        })
        .collect()
}

fn wpa_supplicant_profiles(active_ssid: Option<&str>) -> Vec<StoredWifiProfile> {
    [
        "/etc/wpa_supplicant/wpa_supplicant.conf",
        "/boot/wpa_supplicant.conf",
    ]
    .into_iter()
    .flat_map(|path| wpa_supplicant_profiles_from(Path::new(path), active_ssid))
    .collect()
}

fn wpa_supplicant_profiles_from(path: &Path, active_ssid: Option<&str>) -> Vec<StoredWifiProfile> {
    let Ok(contents) = fs::read_to_string(path) else {
        return Vec::new();
    };

    parse_wpa_supplicant_profiles(&contents, active_ssid, "wpa_supplicant")
}

fn parse_wpa_supplicant_profiles(
    contents: &str,
    active_ssid: Option<&str>,
    source: &str,
) -> Vec<StoredWifiProfile> {
    let mut profiles = Vec::new();
    let mut in_network = false;
    let mut ssid: Option<String> = None;
    let mut key_mgmt: Option<String> = None;
    let mut has_psk = false;
    let mut disabled = false;

    for line in contents.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('#') || trimmed.is_empty() {
            continue;
        }
        if trimmed == "network={" {
            in_network = true;
            ssid = None;
            key_mgmt = None;
            has_psk = false;
            disabled = false;
            continue;
        }
        if in_network && trimmed == "}" {
            if let Some(name) = ssid.take() {
                if !disabled {
                    profiles.push(StoredWifiProfile {
                        active: active_ssid == Some(name.as_str()),
                        name,
                        security: key_mgmt
                            .take()
                            .or_else(|| has_psk.then(|| "WPA-PSK".to_string()))
                            .unwrap_or_else(|| "Open".to_string()),
                        source: source.to_string(),
                        can_delete: true,
                        delete_source: "wpa_supplicant".to_string(),
                    });
                }
            }
            in_network = false;
            continue;
        }
        if !in_network {
            continue;
        }
        if let Some(value) = trimmed.strip_prefix("ssid=") {
            ssid = Some(unquote_wifi_value(value.trim()));
        } else if let Some(value) = trimmed.strip_prefix("key_mgmt=") {
            key_mgmt = Some(value.trim().to_string());
        } else if trimmed.starts_with("psk=") {
            has_psk = true;
        } else if let Some(value) = trimmed.strip_prefix("disabled=") {
            disabled = value.trim() == "1";
        }
    }

    profiles
}

fn dietpi_autosetup_profiles(active_ssid: Option<&str>) -> Vec<StoredWifiProfile> {
    let Ok(contents) = fs::read_to_string("/boot/dietpi.txt") else {
        return Vec::new();
    };
    let fields = key_value_lines(&contents);
    if fields
        .get("AUTO_SETUP_NET_WIFI_ENABLED")
        .map(String::as_str)
        != Some("1")
    {
        return Vec::new();
    }
    let Some(name) = fields
        .get("AUTO_SETUP_NET_WIFI_SSID")
        .map(|value| unquote_wifi_value(value))
        .filter(|value| !value.is_empty())
    else {
        return Vec::new();
    };

    vec![StoredWifiProfile {
        active: active_ssid == Some(name.as_str()),
        name,
        security: "Saved".to_string(),
        source: "DietPi first boot".to_string(),
        can_delete: false,
        delete_source: String::new(),
    }]
}

fn unquote_wifi_value(value: &str) -> String {
    let trimmed = value.trim();
    let unquoted = trimmed
        .strip_prefix('"')
        .and_then(|value| value.strip_suffix('"'))
        .unwrap_or(trimmed);
    let mut decoded = String::new();
    let mut escaped = false;
    for char in unquoted.chars() {
        if escaped {
            decoded.push(char);
            escaped = false;
        } else if char == '\\' {
            escaped = true;
        } else {
            decoded.push(char);
        }
    }
    decoded
}

fn value_after(fields: &[&str], key: &str) -> Option<String> {
    fields
        .iter()
        .position(|field| *field == key)
        .and_then(|index| fields.get(index + 1))
        .map(|value| (*value).to_string())
}

fn frequency_to_channel(frequency_mhz: i32) -> Option<i32> {
    if frequency_mhz == 2484 {
        Some(14)
    } else if (2412..=2472).contains(&frequency_mhz) {
        Some((frequency_mhz - 2407) / 5)
    } else if (5000..=5895).contains(&frequency_mhz) {
        Some((frequency_mhz - 5000) / 5)
    } else if (5955..=7115).contains(&frequency_mhz) {
        Some((frequency_mhz - 5950) / 5)
    } else {
        None
    }
}

fn frequency_band(frequency_mhz: i32) -> String {
    if (2400..2500).contains(&frequency_mhz) {
        "2.4 GHz".to_string()
    } else if (5000..5925).contains(&frequency_mhz) {
        "5 GHz".to_string()
    } else if (5925..7125).contains(&frequency_mhz) {
        "6 GHz".to_string()
    } else {
        format!("{frequency_mhz} MHz")
    }
}

fn wifi_generation_label(value: &str) -> String {
    match value {
        "4" => "Wi-Fi 4 (802.11n)".to_string(),
        "5" => "Wi-Fi 5 (802.11ac)".to_string(),
        "6" => "Wi-Fi 6 (802.11ax)".to_string(),
        "7" => "Wi-Fi 7 (802.11be)".to_string(),
        other => format!("Wi-Fi {other}"),
    }
}

fn service_status(unit: &str) -> ServiceStatus {
    if !command_exists("systemctl") {
        return ServiceStatus {
            unit: unit.to_string(),
            state: "unavailable".to_string(),
            enabled: None,
        };
    }
    let state = systemctl_value(&["is-active", unit]);
    let enabled = systemctl_value(&["is-enabled", unit]);
    ServiceStatus {
        unit: unit.to_string(),
        state: if state.is_empty() {
            "unknown".to_string()
        } else {
            state
        },
        enabled: if enabled.is_empty() || enabled == "unknown" {
            None
        } else {
            Some(enabled)
        },
    }
}

fn systemctl_value(args: &[&str]) -> String {
    run_output("systemctl", args)
        .unwrap_or_else(|| "unknown".to_string())
        .lines()
        .next()
        .unwrap_or("unknown")
        .to_string()
}

fn service_logs(unit: &str, lines: usize) -> Vec<String> {
    let Some(output) = run_output(
        "journalctl",
        &[
            "-u",
            unit,
            "-n",
            &lines.to_string(),
            "--no-pager",
            "--output",
            "short-iso",
        ],
    ) else {
        return Vec::new();
    };
    output
        .lines()
        .filter(|line| !line.is_empty())
        .map(str::to_string)
        .rev()
        .take(lines)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect()
}

fn camera_status() -> CameraStatus {
    let Some(command) = first_available_command(&["rpicam-hello", "libcamera-hello"]) else {
        return CameraStatus {
            available: false,
            tool: None,
            message: "No rpicam/libcamera command found.".to_string(),
        };
    };
    let output = Command::new(&command).arg("--list-cameras").output();
    match output {
        Ok(output) => {
            let message = format!(
                "{}{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            )
            .trim()
            .to_string();
            CameraStatus {
                available: output.status.success() && message.contains("Available cameras"),
                tool: Some(command),
                message,
            }
        }
        Err(error) => CameraStatus {
            available: false,
            tool: Some(command),
            message: error.to_string(),
        },
    }
}

fn wifi_details(wifi: &WifiStatus) -> Vec<LabelValue> {
    let mut rows = Vec::new();
    push_row(&mut rows, "Interface", &wifi.interface);
    push_row(&mut rows, "IP address", &wifi.ip_address);
    push_row(&mut rows, "MAC address", &wifi.mac_address);
    push_row(&mut rows, "BSSID", &wifi.bssid);
    push_row(&mut rows, "Security", &wifi.security);
    push_row(&mut rows, "PHY mode", &wifi.wifi_generation_label);
    if let Some(frequency) = wifi.frequency_mhz {
        let mut value = format!("{frequency} MHz");
        if let Some(band) = &wifi.band {
            value.push_str(&format!(" · {band}"));
        }
        if let Some(channel) = wifi.channel {
            value.push_str(&format!(" · Channel {channel}"));
        }
        rows.push(LabelValue {
            label: "Frequency".to_string(),
            value,
        });
    }
    push_row(&mut rows, "Channel width", &wifi.channel_width);
    push_row(&mut rows, "RSSI", &wifi.signal_dbm);
    push_row(&mut rows, "RX rate", &wifi.rx_bitrate);
    push_row(&mut rows, "TX rate", &wifi.tx_bitrate);
    push_row(&mut rows, "TX power", &wifi.tx_power);
    if let Some(interface) = &wifi.default_interface {
        let value = if let Some(gateway) = &wifi.default_gateway {
            format!("{interface} via {gateway}")
        } else {
            interface.clone()
        };
        rows.push(LabelValue {
            label: "Default route".to_string(),
            value,
        });
    }
    rows
}

fn push_row(rows: &mut Vec<LabelValue>, label: &str, value: &Option<String>) {
    if let Some(value) = value {
        if !value.is_empty() {
            rows.push(LabelValue {
                label: label.to_string(),
                value: value.clone(),
            });
        }
    }
}

fn key_value_lines(output: &str) -> HashMap<String, String> {
    output
        .lines()
        .filter_map(|line| {
            line.split_once('=')
                .map(|(key, value)| (key.trim().to_string(), value.trim().to_string()))
        })
        .collect()
}

fn split_nmcli_fields(value: &str) -> Vec<String> {
    let mut fields = Vec::new();
    let mut current = String::new();
    let mut escaped = false;
    for char in value.chars() {
        if escaped {
            current.push(char);
            escaped = false;
        } else if char == '\\' {
            escaped = true;
        } else if char == ':' {
            fields.push(current);
            current = String::new();
        } else {
            current.push(char);
        }
    }
    fields.push(current);
    fields
}

fn shell_escape(value: &str) -> String {
    value.replace('\'', "'\\''")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_wifi_channels() {
        assert_eq!(frequency_to_channel(2412), Some(1));
        assert_eq!(frequency_band(2412), "2.4 GHz");
    }
}
