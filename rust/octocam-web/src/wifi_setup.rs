use crate::wifi;
use std::{
    env,
    process::{Command, Output},
};

struct SetupConfig {
    ap_ssid: String,
    wifi_iface: String,
}

enum SavedProfileResult {
    Connected,
    NoProfiles,
    Failed,
}

pub fn run() -> Result<(), String> {
    if !command_exists("nmcli") {
        return Err("NetworkManager CLI is required for OctoCam Wi-Fi setup.".to_string());
    }

    let config = SetupConfig::from_env();
    if is_real_wifi_connected(&config)? {
        println!("Wi-Fi is already connected to a saved network; setup AP not needed.");
        return Ok(());
    }

    match try_saved_wifi_profiles(&config)? {
        SavedProfileResult::Connected => Ok(()),
        SavedProfileResult::NoProfiles | SavedProfileResult::Failed => {
            println!("No saved Wi-Fi profile connected successfully; falling back to setup AP.");
            write_captive_dns_config();
            start_setup_ap(&config)
        }
    }
}

impl SetupConfig {
    fn from_env() -> Self {
        Self {
            ap_ssid: env::var("OCTOCAM_SETUP_AP_SSID")
                .unwrap_or_else(|_| "OctoCam-Setup".to_string()),
            wifi_iface: env::var("OCTOCAM_WIFI_IFACE").unwrap_or_else(|_| "wlan0".to_string()),
        }
    }
}

fn is_real_wifi_connected(config: &SetupConfig) -> Result<bool, String> {
    let output = nmcli(["-t", "-f", "NAME,DEVICE", "connection", "show", "--active"])?;
    let text = stdout_text(&output);
    Ok(active_connection_for_iface(&text, &config.wifi_iface)
        .map(|name| name != config.ap_ssid)
        .unwrap_or(false))
}

fn try_saved_wifi_profiles(config: &SetupConfig) -> Result<SavedProfileResult, String> {
    let profiles = saved_wifi_profiles_by_last_connected(&config.ap_ssid)?;
    if profiles.is_empty() {
        return Ok(SavedProfileResult::NoProfiles);
    }

    let mut connected = false;
    for profile in profiles {
        println!("Trying saved Wi-Fi profile: {profile}");
        let _ = nmcli([
            "connection",
            "modify",
            &config.ap_ssid,
            "connection.autoconnect",
            "no",
        ]);

        let output = crate::proc::run(
            Command::new("nmcli").args(["connection", "up", &profile, "ifname", &config.wifi_iface]),
            crate::proc::CONNECT_TIMEOUT,
        )
        .map_err(|error| error.to_string())?;
        if output.status.success() {
            println!("Connected using saved Wi-Fi profile: {profile}");
            connected = true;
            break;
        }
    }

    Ok(if connected {
        SavedProfileResult::Connected
    } else {
        SavedProfileResult::Failed
    })
}

fn saved_wifi_profiles_by_last_connected(ap_ssid: &str) -> Result<Vec<String>, String> {
    let output = nmcli(["-g", "NAME", "connection", "show"])?;
    let mut profiles = Vec::new();

    for name in stdout_text(&output).lines().map(str::trim) {
        if name.is_empty() || name == ap_ssid {
            continue;
        }
        if connection_type(name).as_deref() != Some("802-11-wireless") {
            continue;
        }
        profiles.push((connection_timestamp(name), name.to_string()));
    }

    profiles.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(&b.1)));
    Ok(profiles.into_iter().map(|(_, name)| name).collect())
}

/// NM's shared-mode dnsmasq reads this drop-in dir; the wildcard makes every DNS
/// name resolve to the AP gateway so OS captive probes actually reach our port-80
/// listener. Only affects the dnsmasq instance NM spawns for shared (AP)
/// connections — normal client-mode DNS is untouched. Best-effort.
fn write_captive_dns_config() {
    let _ = std::fs::create_dir_all("/etc/NetworkManager/dnsmasq-shared.d");
    let _ = std::fs::write(
        "/etc/NetworkManager/dnsmasq-shared.d/octocam-captive.conf",
        "# OctoCam setup AP: resolve everything to the gateway so captive probes reach us.\naddress=/#/10.42.0.1\n",
    );
}

fn start_setup_ap(config: &SetupConfig) -> Result<(), String> {
    println!("Scanning and starting setup AP.");
    if let Err(error) = wifi::scan_and_cache_networks(&wifi::default_cache_path()) {
        println!("Wi-Fi scan failed before setup AP start: {error}");
    }

    if connection_exists(&config.ap_ssid)? {
        nmcli([
            "connection",
            "modify",
            &config.ap_ssid,
            "connection.autoconnect",
            "yes",
        ])?;
        nmcli(["connection", "up", &config.ap_ssid])?;
        return Ok(());
    }

    nmcli([
        "connection",
        "add",
        "type",
        "wifi",
        "ifname",
        &config.wifi_iface,
        "con-name",
        &config.ap_ssid,
        "autoconnect",
        "yes",
        "ssid",
        &config.ap_ssid,
    ])?;
    nmcli([
        "connection",
        "modify",
        &config.ap_ssid,
        "802-11-wireless.mode",
        "ap",
        "802-11-wireless.band",
        "bg",
        "ipv4.method",
        "shared",
        "ipv6.method",
        "disabled",
    ])?;
    nmcli(["connection", "up", &config.ap_ssid])?;
    Ok(())
}

fn connection_exists(name: &str) -> Result<bool, String> {
    let output = nmcli(["-t", "-f", "NAME", "connection", "show"])?;
    Ok(stdout_text(&output).lines().any(|line| line.trim() == name))
}

fn connection_type(name: &str) -> Option<String> {
    nmcli(["-g", "connection.type", "connection", "show", name])
        .ok()
        .map(|output| stdout_text(&output).trim().to_string())
        .filter(|value| !value.is_empty())
}

fn connection_timestamp(name: &str) -> u64 {
    nmcli(["-g", "connection.timestamp", "connection", "show", name])
        .ok()
        .and_then(|output| stdout_text(&output).trim().parse::<u64>().ok())
        .unwrap_or(0)
}

fn active_connection_for_iface(output: &str, iface: &str) -> Option<String> {
    output.lines().find_map(|line| {
        let fields = wifi::split_escaped(line);
        if fields.len() >= 2 && fields[1] == iface {
            Some(fields[0].clone())
        } else {
            None
        }
    })
}

fn nmcli<const N: usize>(args: [&str; N]) -> Result<Output, String> {
    let output = crate::proc::run(Command::new("nmcli").args(args), crate::proc::CONNECT_TIMEOUT)
        .map_err(|error| error.to_string())?;
    if output.status.success() {
        Ok(output)
    } else {
        Err(error_text(&output).trim().to_string())
    }
}

fn stdout_text(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).to_string()
}

fn error_text(output: &Output) -> String {
    let bytes = if output.stderr.is_empty() {
        &output.stdout
    } else {
        &output.stderr
    };
    String::from_utf8_lossy(bytes).to_string()
}

fn command_exists(command: &str) -> bool {
    crate::proc::run(
        Command::new("sh").args(["-c", &format!("command -v {command} >/dev/null 2>&1")]),
        crate::proc::DEFAULT_TIMEOUT,
    )
    .map(|output| output.status.success())
    .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_active_connection_for_interface() {
        let output = "OctoCam-Setup:wlan0\nEthernet:eth0\n";
        assert_eq!(
            active_connection_for_iface(output, "wlan0").as_deref(),
            Some("OctoCam-Setup")
        );
    }

    #[test]
    fn ignores_other_interfaces() {
        let output = "Office WiFi:wlan1\nEthernet:eth0\n";
        assert_eq!(active_connection_for_iface(output, "wlan0"), None);
    }
}
