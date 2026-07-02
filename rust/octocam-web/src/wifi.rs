use serde::{Deserialize, Serialize};
use std::{
    env, fs, io,
    path::PathBuf,
    process::Command,
    time::{SystemTime, UNIX_EPOCH},
};

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub struct WifiCache {
    pub scanned_at: Option<u64>,
    pub networks: Vec<WifiNetwork>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct WifiNetwork {
    pub ssid: String,
    pub security: String,
    pub raw_security: String,
    pub signal: i32,
}

#[derive(Clone, Debug)]
pub struct WifiNetworkView {
    pub ssid: String,
    pub label: String,
    pub security: String,
    pub selected: bool,
}

pub fn default_cache_path() -> PathBuf {
    env::var_os("OCTOCAM_WIFI_CACHE_PATH")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            let home = env::var_os("HOME")
                .map(PathBuf::from)
                .unwrap_or_else(|| PathBuf::from("."));
            home.join(".cache/octocam/wifi-networks.json")
        })
}

pub fn load_network_cache(path: &PathBuf) -> WifiCache {
    let Ok(raw) = fs::read_to_string(path) else {
        return WifiCache::default();
    };
    let Ok(cache) = serde_json::from_str::<WifiCache>(&raw) else {
        return WifiCache::default();
    };
    WifiCache {
        scanned_at: cache.scanned_at,
        networks: cache
            .networks
            .into_iter()
            .filter(valid_cached_network)
            .collect(),
    }
}

pub fn save_network_cache(path: &PathBuf, networks: &[WifiNetwork]) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let scanned_at = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let cache = WifiCache {
        scanned_at: Some(scanned_at),
        networks: networks.to_vec(),
    };
    fs::write(path, format!("{}\n", serde_json::to_string_pretty(&cache)?))
}

pub fn scan_and_cache_networks(path: &PathBuf) -> Result<WifiCache, String> {
    let networks = scan_networks()?;
    save_network_cache(path, &networks).map_err(|error| error.to_string())?;
    Ok(load_network_cache(path))
}

pub fn scan_networks() -> Result<Vec<WifiNetwork>, String> {
    match scan_networks_with_nmcli() {
        Ok(networks) if !networks.is_empty() => Ok(networks),
        Ok(_) | Err(_) => scan_networks_with_iw(),
    }
}

fn scan_networks_with_nmcli() -> Result<Vec<WifiNetwork>, String> {
    let output = Command::new("nmcli")
        .args([
            "-t",
            "-f",
            "SSID,SECURITY,SIGNAL",
            "dev",
            "wifi",
            "list",
            "--rescan",
            "yes",
        ])
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
    Ok(dedupe_networks(parse_nmcli_wifi_list(
        &String::from_utf8_lossy(&output.stdout),
    )))
}

fn scan_networks_with_iw() -> Result<Vec<WifiNetwork>, String> {
    let interfaces = wireless_interfaces();
    let mut last_error = "No wireless interface found.".to_string();
    for interface in interfaces {
        let output = Command::new("iw")
            .args(["dev", &interface, "scan"])
            .output()
            .map_err(|error| error.to_string())?;
        if output.status.success() {
            return Ok(dedupe_networks(parse_iw_scan(&String::from_utf8_lossy(
                &output.stdout,
            ))));
        }
        let message = String::from_utf8_lossy(if output.stderr.is_empty() {
            &output.stdout
        } else {
            &output.stderr
        })
        .trim()
        .to_string();
        if !message.is_empty() {
            last_error = message;
        }
    }
    Err(last_error)
}

pub fn connect_to_network(ssid: &str, password: &str, security: &str) -> (bool, String) {
    let ssid = ssid.trim();
    let security = normalize_security(security);
    if ssid.is_empty() {
        return (false, "Missing Wi-Fi network name.".to_string());
    }

    let mut command = Command::new("nmcli");
    command.args(["dev", "wifi", "connect", ssid]);
    if security != "open" {
        if password.is_empty() {
            return (
                false,
                format!("{} network requires a password.", security.to_uppercase()),
            );
        }
        command.args(["password", password]);
    }

    let nmcli_result = run_connect_command(command);
    if nmcli_result.0 {
        disable_setup_ap();
        return nmcli_result;
    }

    let wpa_result = connect_with_wpa_cli(ssid, password, &security);
    if wpa_result.0 {
        disable_setup_ap();
        return wpa_result;
    }

    (
        false,
        format!(
            "NetworkManager: {} wpa_supplicant: {}",
            nmcli_result.1, wpa_result.1
        ),
    )
}

pub fn forget_saved_profile(name: &str, source: &str) -> (bool, String) {
    let name = name.trim();
    if name.is_empty() {
        return (false, "Missing Wi-Fi profile name.".to_string());
    }
    match source {
        "network_manager" => forget_network_manager_profile(name),
        "wpa_supplicant" => forget_wpa_supplicant_profile(name),
        _ => (
            false,
            "This Wi-Fi profile cannot be deleted here.".to_string(),
        ),
    }
}

fn forget_network_manager_profile(name: &str) -> (bool, String) {
    run_connect_command({
        let mut command = Command::new("nmcli");
        command.args(["connection", "delete", "id", name]);
        command
    })
}

fn forget_wpa_supplicant_profile(name: &str) -> (bool, String) {
    let Some(interface) = wireless_interfaces().into_iter().next() else {
        return (false, "No wireless interface found.".to_string());
    };
    let (listed, networks) = run_wpa_cli(&interface, &["list_networks"]);
    if !listed {
        return (false, networks);
    }
    let Some(network_id) = wpa_network_id_for_ssid(&networks, name) else {
        return (
            false,
            format!("No saved wpa_supplicant profile named {name}."),
        );
    };
    let (removed, remove_message) = run_wpa_cli(&interface, &["remove_network", &network_id]);
    if !removed {
        return (false, remove_message);
    }
    let (saved, save_message) = run_wpa_cli(&interface, &["save_config"]);
    if !saved {
        return (false, save_message);
    }
    (true, format!("Deleted Wi-Fi profile {name}."))
}

fn wpa_network_id_for_ssid(output: &str, ssid: &str) -> Option<String> {
    output.lines().skip(1).find_map(|line| {
        let fields: Vec<&str> = line.split('\t').collect();
        if fields.len() >= 2 && fields[1] == ssid {
            Some(fields[0].to_string())
        } else {
            None
        }
    })
}

fn run_connect_command(mut command: Command) -> (bool, String) {
    match command.output() {
        Ok(output) => {
            let text = if output.stdout.is_empty() {
                output.stderr
            } else {
                output.stdout
            };
            let message = String::from_utf8_lossy(&text).trim().to_string();
            (
                output.status.success(),
                if message.is_empty() {
                    "Command returned no output.".to_string()
                } else {
                    message
                },
            )
        }
        Err(error) => (false, error.to_string()),
    }
}

fn connect_with_wpa_cli(ssid: &str, password: &str, security: &str) -> (bool, String) {
    let Some(interface) = wireless_interfaces().into_iter().next() else {
        return (false, "No wireless interface found.".to_string());
    };
    let (added, network_id) = run_wpa_cli(&interface, &["add_network"]);
    if !added {
        return (false, network_id);
    }
    let network_id = network_id.trim();
    if network_id.is_empty() {
        return (false, "wpa_cli returned no network id.".to_string());
    }

    let ssid_value = quoted_wpa_value(ssid);
    let (ssid_set, ssid_message) = run_wpa_cli(
        &interface,
        &["set_network", network_id, "ssid", &ssid_value],
    );
    if !ssid_set {
        return (false, ssid_message);
    }

    if security == "open" {
        let (key_set, key_message) =
            run_wpa_cli(&interface, &["set_network", network_id, "key_mgmt", "NONE"]);
        if !key_set {
            return (false, key_message);
        }
    } else {
        let psk_value = quoted_wpa_value(password);
        let (psk_set, psk_message) =
            run_wpa_cli(&interface, &["set_network", network_id, "psk", &psk_value]);
        if !psk_set {
            return (false, psk_message);
        }
    }

    for args in [
        vec!["enable_network", network_id],
        vec!["save_config"],
        vec!["reconfigure"],
    ] {
        let (success, message) = run_wpa_cli(&interface, &args);
        if !success {
            return (false, message);
        }
    }

    (
        true,
        format!("Saved {ssid} to wpa_supplicant on {interface}."),
    )
}

fn run_wpa_cli(interface: &str, args: &[&str]) -> (bool, String) {
    let output = Command::new("wpa_cli")
        .arg("-i")
        .arg(interface)
        .args(args)
        .output();
    match output {
        Ok(output) => {
            let text = if output.stdout.is_empty() {
                output.stderr
            } else {
                output.stdout
            };
            let message = String::from_utf8_lossy(&text).trim().to_string();
            let success = output.status.success()
                && !matches!(
                    message.as_str(),
                    "FAIL" | "UNKNOWN COMMAND" | "INVALID COMMAND"
                );
            (success, message)
        }
        Err(error) => (false, error.to_string()),
    }
}

fn quoted_wpa_value(value: &str) -> String {
    format!("\"{}\"", value.replace('\\', "\\\\").replace('"', "\\\""))
}

pub fn cached_security_for(cache: &WifiCache, ssid: &str) -> String {
    cache
        .networks
        .iter()
        .find(|network| network.ssid == ssid)
        .map(|network| network.security.clone())
        .unwrap_or_else(|| "wpa2".to_string())
}

pub fn network_views(cache: &WifiCache, selected_ssid: &str) -> Vec<WifiNetworkView> {
    cache
        .networks
        .iter()
        .map(|network| WifiNetworkView {
            ssid: network.ssid.clone(),
            label: format!(
                "{} · {} · {}%",
                network.ssid,
                network.security.to_uppercase(),
                network.signal
            ),
            security: network.security.clone(),
            selected: network.ssid == selected_ssid,
        })
        .collect()
}

pub fn parse_nmcli_wifi_list(output: &str) -> Vec<WifiNetwork> {
    output
        .lines()
        .filter_map(|line| {
            if line.is_empty() {
                return None;
            }
            let fields = split_escaped(line);
            if fields.len() < 3 {
                return None;
            }
            let ssid = unescape_nmcli(&fields[0]).trim().to_string();
            if ssid.is_empty() {
                return None;
            }
            let raw_security = unescape_nmcli(&fields[1]).trim().to_string();
            Some(WifiNetwork {
                ssid,
                security: normalize_security(&raw_security),
                raw_security,
                signal: parse_signal(&fields[2]),
            })
        })
        .collect()
}

pub fn parse_iw_scan(output: &str) -> Vec<WifiNetwork> {
    let mut networks = Vec::new();
    let mut ssid: Option<String> = None;
    let mut raw_security = String::new();
    let mut signal = 0;

    for line in output.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("BSS ") {
            push_iw_network(&mut networks, ssid.take(), &raw_security, signal);
            raw_security.clear();
            signal = 0;
            continue;
        }
        if let Some(value) = trimmed.strip_prefix("SSID: ") {
            ssid = Some(value.trim().to_string());
        } else if let Some(value) = trimmed.strip_prefix("signal: ") {
            signal = signal_dbm_to_percent(value);
        } else if trimmed.starts_with("RSN:") {
            raw_security.push_str(" RSN");
        } else if trimmed.starts_with("WPA:") {
            raw_security.push_str(" WPA");
        } else if trimmed.contains("Privacy") {
            raw_security.push_str(" WEP");
        }
    }
    push_iw_network(&mut networks, ssid.take(), &raw_security, signal);
    networks
}

fn push_iw_network(
    networks: &mut Vec<WifiNetwork>,
    ssid: Option<String>,
    raw_security: &str,
    signal: i32,
) {
    let Some(ssid) = ssid.map(|value| value.trim().to_string()) else {
        return;
    };
    if ssid.is_empty() {
        return;
    }
    let raw_security = raw_security.trim().to_string();
    networks.push(WifiNetwork {
        ssid,
        security: normalize_security(&raw_security),
        raw_security,
        signal,
    });
}

pub fn normalize_security(value: &str) -> String {
    let normalized = value.to_uppercase().replace(['-', '_'], "");
    if normalized.is_empty() || normalized == "--" {
        "open".to_string()
    } else if normalized.contains("WPA3") || normalized.contains("SAE") {
        "wpa3".to_string()
    } else if normalized.contains("WPA2") || normalized.contains("RSN") {
        "wpa2".to_string()
    } else if normalized.contains("WPA") {
        "wpa".to_string()
    } else if normalized.contains("WEP") {
        "wep".to_string()
    } else {
        "unknown".to_string()
    }
}

pub fn split_escaped(value: &str) -> Vec<String> {
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

fn disable_setup_ap() {
    let ap_ssid = env::var("OCTOCAM_SETUP_AP_SSID").unwrap_or_else(|_| "OctoCam-Setup".to_string());
    let _ = Command::new("nmcli")
        .args([
            "connection",
            "modify",
            &ap_ssid,
            "connection.autoconnect",
            "no",
        ])
        .output();
    let _ = Command::new("nmcli")
        .args(["connection", "down", &ap_ssid])
        .output();
}

fn dedupe_networks(networks: Vec<WifiNetwork>) -> Vec<WifiNetwork> {
    let mut best = std::collections::BTreeMap::<String, WifiNetwork>::new();
    for network in networks {
        let key = network.ssid.clone();
        if best
            .get(&key)
            .map(|existing| network.signal > existing.signal)
            .unwrap_or(true)
        {
            best.insert(key, network);
        }
    }
    let mut networks: Vec<WifiNetwork> = best.into_values().collect();
    networks.sort_by(|a, b| {
        b.signal
            .cmp(&a.signal)
            .then_with(|| a.ssid.to_lowercase().cmp(&b.ssid.to_lowercase()))
    });
    networks
}

fn parse_signal(value: &str) -> i32 {
    value.parse::<i32>().unwrap_or(0).clamp(0, 100)
}

fn signal_dbm_to_percent(value: &str) -> i32 {
    let dbm = value
        .split_whitespace()
        .next()
        .and_then(|value| value.parse::<f64>().ok())
        .unwrap_or(-100.0);
    (((dbm + 100.0) / 50.0) * 100.0).round().clamp(0.0, 100.0) as i32
}

fn unescape_nmcli(value: &str) -> String {
    value.replace("\\:", ":").replace("\\\\", "\\")
}

fn wireless_interfaces() -> Vec<String> {
    env::var("OCTOCAM_WIFI_INTERFACE")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .map(|value| vec![value])
        .unwrap_or_else(|| {
            fs::read_dir("/sys/class/net")
                .ok()
                .into_iter()
                .flat_map(|entries| entries.flatten())
                .filter_map(|entry| entry.file_name().into_string().ok())
                .filter(|name| name.starts_with("wlan") || name.starts_with("wl"))
                .collect()
        })
}

fn valid_cached_network(network: &WifiNetwork) -> bool {
    matches!(
        network.security.as_str(),
        "open" | "wep" | "wpa" | "wpa2" | "wpa3" | "unknown"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_escaped_nmcli_lines() {
        let networks = parse_nmcli_wifi_list("Roost\\:Up:WPA2:83\nOpenNet::44\n");
        assert_eq!(networks[0].ssid, "Roost:Up");
        assert_eq!(networks[0].security, "wpa2");
        assert_eq!(networks[1].security, "open");
    }

    #[test]
    fn normalizes_security() {
        assert_eq!(normalize_security("WPA2 WPA3 SAE"), "wpa3");
        assert_eq!(normalize_security("--"), "open");
        assert_eq!(normalize_security("WEP"), "wep");
    }

    #[test]
    fn parses_iw_scan_blocks() {
        let networks = parse_iw_scan(
            "BSS 60:22:32:ee:d5:2a(on wlan0)\n\
             \tsignal: -57.00 dBm\n\
             \tSSID: RoostUp-141-1\n\
             \tRSN:\n\
             BSS aa:bb:cc:dd:ee:ff(on wlan0)\n\
             \tsignal: -81.00 dBm\n\
             \tSSID: Guest\n",
        );
        assert_eq!(networks[0].ssid, "RoostUp-141-1");
        assert_eq!(networks[0].security, "wpa2");
        assert_eq!(networks[0].signal, 86);
        assert_eq!(networks[1].security, "open");
    }
}
