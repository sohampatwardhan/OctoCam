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

    match command.output() {
        Ok(output) => {
            let text = if output.stdout.is_empty() {
                output.stderr
            } else {
                output.stdout
            };
            let message = String::from_utf8_lossy(&text).trim().to_string();
            if output.status.success() {
                disable_setup_ap();
            }
            (
                output.status.success(),
                if message.is_empty() {
                    "NetworkManager returned no output.".to_string()
                } else {
                    message
                },
            )
        }
        Err(error) => (false, error.to_string()),
    }
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

fn unescape_nmcli(value: &str) -> String {
    value.replace("\\:", ":").replace("\\\\", "\\")
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
}
