const copyButtons = document.querySelectorAll("[data-copy-target]");
const STREAM_PREVIEW_CACHE_KEY = "octocam.streamPreview";
const LIVE_REFRESH_MS = 5000;
const MOBILE_NAV_QUERY = "(max-width: 760px)";

if ("serviceWorker" in navigator) {
  window.addEventListener("load", () => {
    navigator.serviceWorker.register("/sw.js").catch(() => {});
  });
}

async function writeClipboard(text) {
  try {
    if (navigator.clipboard && window.isSecureContext) {
      await navigator.clipboard.writeText(text);
      return true;
    }
  } catch (error) {
  }

  const buffer = document.createElement("textarea");
  buffer.value = text;
  buffer.setAttribute("readonly", "");
  buffer.style.position = "fixed";
  buffer.style.inset = "0 auto auto 0";
  buffer.style.opacity = "0";
  document.body.appendChild(buffer);
  buffer.select();
  const copied = document.execCommand("copy");
  buffer.remove();
  return copied;
}

function selectTarget(target) {
  target.focus();
  target.select();
  target.setSelectionRange(0, target.value.length);
}

async function copyValue(button) {
  const target = document.getElementById(button.dataset.copyTarget);
  if (!target) {
    return;
  }

  const copied = await writeClipboard(target.value);
  if (!copied) {
    selectTarget(target);
  }

  button.dataset.copied = copied ? "true" : "selected";
  window.setTimeout(() => {
    delete button.dataset.copied;
  }, 1600);
}

copyButtons.forEach((button) => {
  button.addEventListener("click", () => copyValue(button));
});

const menuButton = document.querySelector("[data-menu-button]");
const sideNav = document.querySelector("[data-side-nav]");
const mobileNav = window.matchMedia(MOBILE_NAV_QUERY);

function setMenuOpen(open) {
  if (!menuButton || !sideNav) {
    return;
  }

  sideNav.classList.toggle("is-open", open);
  menuButton.setAttribute("aria-expanded", open ? "true" : "false");
  menuButton.setAttribute("aria-label", open ? "Close menu" : "Open menu");
}

function syncMenuForViewport() {
  if (!menuButton || !sideNav) {
    return;
  }

  if (!mobileNav.matches) {
    setMenuOpen(false);
  }
}

if (menuButton && sideNav) {
  menuButton.addEventListener("click", () => {
    setMenuOpen(!sideNav.classList.contains("is-open"));
  });

  sideNav.querySelectorAll("a").forEach((link) => {
    link.addEventListener("click", () => {
      if (mobileNav.matches) {
        setMenuOpen(false);
      }
    });
  });

  if (mobileNav.addEventListener) {
    mobileNav.addEventListener("change", syncMenuForViewport);
  } else if (mobileNav.addListener) {
    mobileNav.addListener(syncMenuForViewport);
  }

  syncMenuForViewport();
}

const liveFields = document.querySelectorAll("[data-live-status]");
const liveMeters = document.querySelectorAll("[data-live-meter]");
const liveSignal = document.querySelector("[data-live-signal]");
const liveLogs = document.querySelector("[data-live-logs]");
const wifiDetails = document.querySelectorAll("[data-wifi-details]");
const powerButton = document.querySelector("[data-power-button]");
const powerDialog = document.querySelector("[data-power-dialog]");
const powerDialogBackdrop = document.querySelector("[data-power-dialog-backdrop]");
const powerDialogOpen = document.querySelector("[data-power-dialog-open]");
const powerDialogCloseButtons = document.querySelectorAll("[data-power-dialog-close]");
const powerOptionForms = document.querySelectorAll("[data-power-option-form]");
const wifiDialog = document.querySelector("[data-wifi-dialog]");
const wifiDialogBackdrop = document.querySelector("[data-wifi-dialog-backdrop]");
const wifiDialogOpen = document.querySelector("[data-wifi-dialog-open]");
const wifiDialogCloseButtons = document.querySelectorAll("[data-wifi-dialog-close]");
const wifiModeButtons = document.querySelectorAll("[data-wifi-mode]");
const wifiScanPanel = document.querySelector("[data-wifi-scan-panel]");
const wifiManualPanel = document.querySelector("[data-wifi-manual-panel]");
const wifiNetworkSelect = document.querySelector("[data-wifi-network-select]");
const wifiManualSsid = document.querySelector("[data-wifi-manual-ssid]");
const wifiSsidValue = document.querySelector("[data-wifi-ssid-value]");
const wifiSecuritySelect = document.querySelector("[data-wifi-security-select]");
const wifiPasswordRow = document.querySelector("[data-wifi-password-row]");
const wifiPasswordInput = document.querySelector("[data-wifi-password-input]");
const wifiScanButton = document.querySelector("[data-wifi-scan-button]");
const wifiScanStatus = document.querySelector("[data-wifi-scan-status]");
const wifiConnectForm = document.querySelector("[data-wifi-connect-form]");
let liveRefreshTimer = null;
let liveRefreshPending = false;
let wifiMode = "scan";

function present(value, fallback = "Not available") {
  if (value === null || value === undefined || value === "") {
    return fallback;
  }
  return String(value);
}

function fixed(value, suffix) {
  if (typeof value !== "number" || !Number.isFinite(value)) {
    return "Not available";
  }
  return `${value.toFixed(1)}${suffix}`;
}

function memoryText(memory) {
  if (!memory || !memory.total_mb) {
    return "Not available";
  }
  const percent = typeof memory.used_percent === "number"
    ? ` (${memory.used_percent.toFixed(1)}%)`
    : "";
  return `${memory.used_mb} / ${memory.total_mb} MB${percent}`;
}

function swapText(memory) {
  if (!memory || !memory.swap_total_mb) {
    return "Not available";
  }
  const percent = typeof memory.swap_used_percent === "number"
    ? ` (${memory.swap_used_percent.toFixed(1)}%)`
    : "";
  return `${memory.swap_used_mb} / ${memory.swap_total_mb} MB${percent}`;
}

function clampPercent(value) {
  if (typeof value !== "number" || !Number.isFinite(value)) {
    return 0;
  }
  return Math.max(0, Math.min(100, value));
}

function parseSignalDbm(signal) {
  if (!signal) {
    return null;
  }
  const value = Number.parseFloat(String(signal).split(/\s+/)[0]);
  return Number.isFinite(value) ? value : null;
}

function signalPercent(wifi) {
  const dbm = parseSignalDbm(wifi?.signal_dbm);
  if (dbm === null) {
    return 0;
  }
  return clampPercent(((dbm + 100) / 50) * 100);
}

function signalLevel(percent) {
  if (percent >= 67) {
    return "high";
  }
  if (percent >= 34) {
    return "low";
  }
  return "zero";
}

function signalLabel(wifi) {
  const percent = signalPercent(wifi);
  return wifi?.signal_dbm
    ? `Signal ${wifi.signal_dbm} (${percent.toFixed(0)}%)`
    : "Signal unavailable";
}

function wifiLabel(status) {
  return present(status?.wifi?.ssid, present(status?.wifi?.message));
}

function ipAddresses(status) {
  if (!Array.isArray(status?.ip_addresses) || status.ip_addresses.length === 0) {
    return "Not available";
  }
  return status.ip_addresses.join(", ");
}

function wifiIpAddresses(status) {
  if (Array.isArray(status?.wifi?.ip_addresses) && status.wifi.ip_addresses.length > 0) {
    return status.wifi.ip_addresses.join(", ");
  }
  return present(status?.wifi?.ip_address);
}

function statusValues(status) {
  return {
    camera_label: status?.camera?.available ? "Camera online" : "Camera unavailable",
    wifi_label: wifiLabel(status),
    wifi_ip_addresses: wifiIpAddresses(status),
    ip_addresses: ipAddresses(status),
    uptime: present(status?.uptime),
    cpu_temp: fixed(status?.cpu_temp_c, " C"),
    cpu_usage: fixed(status?.resources?.cpu_usage_percent, "%"),
    load_average: present(status?.resources?.load_average),
    memory: present(status?.resources?.memory_summary, memoryText(status?.resources?.memory)),
    swap: swapText(status?.resources?.memory),
    web_state: present(status?.services?.octocam_web?.state),
    rtsp_state: present(status?.services?.rtsp?.state),
    homekit_state: present(status?.services?.homekit?.state),
  };
}

function updateText(node, value) {
  const next = present(value);
  if (node.textContent !== next) {
    node.textContent = next;
  }
}

function wifiDetailRows(wifi) {
  if (!wifi) {
    return [];
  }
  return [
    ["Interface", wifi.interface],
    ["BSSID", wifi.bssid],
    ["Band", wifi.band],
    ["Channel", wifi.channel],
    ["Width", wifi.channel_width],
    ["Signal", wifi.signal_dbm],
    ["RX bitrate", wifi.rx_bitrate],
    ["TX bitrate", wifi.tx_bitrate],
    ["Security", wifi.security],
    ["IP address", wifi.ip_address],
    ["MAC address", wifi.mac_address],
    ["Gateway", wifi.default_gateway],
    ["Source", wifi.source],
  ].filter((row) => row[1] !== null && row[1] !== undefined && row[1] !== "");
}

function renderWifiDetails(status) {
  if (!wifiDetails.length) {
    return;
  }
  const rows = wifiDetailRows(status?.wifi);
  wifiDetails.forEach((list) => {
    list.replaceChildren();
    rows.forEach(([label, value]) => {
      const row = document.createElement("div");
      const term = document.createElement("dt");
      const definition = document.createElement("dd");
      term.textContent = label;
      definition.textContent = String(value);
      row.append(term, definition);
      list.append(row);
    });
  });
}

function updateMeters(status) {
  if (!liveMeters.length) {
    return;
  }
  const values = {
    cpu_usage: clampPercent(status?.resources?.cpu_usage_percent),
    memory: clampPercent(status?.resources?.memory?.used_percent),
    swap: clampPercent(status?.resources?.memory?.swap_used_percent),
  };
  liveMeters.forEach((meter) => {
    const value = values[meter.dataset.liveMeter] ?? 0;
    meter.style.setProperty("--meter-value", `${value}%`);
  });
}

function updateWifiSignal(status) {
  if (!liveSignal) {
    return;
  }
  const percent = signalPercent(status?.wifi);
  const label = signalLabel(status?.wifi);
  liveSignal.dataset.signalLevel = signalLevel(percent);
  liveSignal.setAttribute("title", label);
  const indicator = liveSignal.querySelector(".wifi-signal-indicator");
  if (indicator) {
    indicator.setAttribute("aria-label", label);
  }
}

function updatePower(settings, status) {
  if (!powerButton) {
    return;
  }

  const enabled = settings?.camera_enabled !== false;
  const available = status?.camera?.available === true;
  powerButton.classList.toggle("is-on", enabled);
  powerButton.classList.toggle("is-off", !enabled);
  powerButton.classList.toggle("is-available", available);
  powerButton.classList.toggle("is-unavailable", !available);

  const label = enabled ? "Turn OctoCam off" : "Turn OctoCam on";
  powerButton.setAttribute("aria-label", `Open OctoCam power options. ${label}.`);
  powerButton.setAttribute("title", "Open OctoCam power options");
}

function applyLiveState({ settings, status }) {
  if (status) {
    const values = statusValues(status);
    liveFields.forEach((field) => {
      updateText(field, values[field.dataset.liveStatus]);
    });
    renderWifiDetails(status);
    updateMeters(status);
    updateWifiSignal(status);

    if (liveLogs) {
      updateText(
        liveLogs,
        Array.isArray(status.logs) && status.logs.length
          ? `${status.logs.join("\n")}\n`
          : "No recent logs available.",
      );
    }
  }

  if (settings || status) {
    updatePower(settings, status);
  }
}

async function fetchJson(path, options = {}) {
  const response = await fetch(path, {
    ...options,
    credentials: "same-origin",
    headers: { Accept: "application/json", ...(options.headers || {}) },
  });
  if (!response.ok) {
    throw new Error(`Request failed: ${path}`);
  }
  return response.json();
}

async function refreshLiveState() {
  if (liveRefreshPending || document.hidden) {
    return;
  }
  liveRefreshPending = true;
  try {
    const [settings, status] = await Promise.all([
      fetchJson("/api/settings"),
      fetchJson("/api/status"),
    ]);
    applyLiveState({ settings, status });
  } catch (error) {
  } finally {
    liveRefreshPending = false;
  }
}

function currentReturnPath() {
  const path = window.location.pathname || "/identity";
  if (!path.startsWith("/") || path.startsWith("//")) {
    return "/identity";
  }
  return path;
}

function syncPowerReturnPaths() {
  powerOptionForms.forEach((form) => {
    const input = form.querySelector("[data-return-to]");
    if (input) {
      input.value = currentReturnPath();
    }
  });
}

function openPowerDialog() {
  if (!powerDialog) {
    return;
  }
  syncPowerReturnPaths();
  powerDialog.hidden = false;
  powerDialog.classList.add("is-open");
  if (powerDialogBackdrop) {
    powerDialogBackdrop.hidden = false;
  }
  document.body.classList.add("power-dialog-open");
  powerDialog.querySelector("[data-power-dialog-close]")?.focus();
}

function closePowerDialog() {
  if (!powerDialog) {
    return;
  }
  powerDialog.hidden = true;
  powerDialog.classList.remove("is-open");
  if (powerDialogBackdrop) {
    powerDialogBackdrop.hidden = true;
  }
  document.body.classList.remove("power-dialog-open");
  powerDialogOpen?.focus();
}

if (powerDialogOpen && powerDialog) {
  powerDialogOpen.addEventListener("click", openPowerDialog);
}

powerDialogCloseButtons.forEach((button) => {
  button.addEventListener("click", closePowerDialog);
});

if (powerDialog) {
  document.addEventListener("keydown", (event) => {
    if (event.key === "Escape" && !powerDialog.hidden) {
      closePowerDialog();
    }
  });
}

if (powerDialogBackdrop) {
  powerDialogBackdrop.addEventListener("click", closePowerDialog);
}

powerOptionForms.forEach((form) => {
  form.addEventListener("submit", () => {
    syncPowerReturnPaths();
    form.querySelectorAll("button").forEach((button) => {
      button.disabled = true;
    });
  });
});

function syncWifiSsidValue() {
  if (!wifiSsidValue) {
    return;
  }
  wifiSsidValue.value = wifiMode === "manual"
    ? (wifiManualSsid?.value || "")
    : (wifiNetworkSelect?.value || "");
}

function selectedScanSecurity() {
  const option = wifiNetworkSelect?.selectedOptions?.[0];
  return option?.dataset.security || "";
}

function syncWifiSecurityFromScan() {
  if (wifiMode !== "scan" || !wifiSecuritySelect) {
    return;
  }
  const security = selectedScanSecurity();
  if (security) {
    wifiSecuritySelect.value = security;
  }
  syncWifiPasswordRequirement();
}

function syncWifiPasswordRequirement() {
  const isOpen = wifiSecuritySelect?.value === "open";
  if (wifiPasswordRow) {
    wifiPasswordRow.hidden = isOpen;
  }
  if (wifiPasswordInput) {
    wifiPasswordInput.disabled = isOpen;
    if (isOpen) {
      wifiPasswordInput.value = "";
    }
  }
}

function setWifiMode(mode) {
  wifiMode = mode === "manual" ? "manual" : "scan";
  wifiModeButtons.forEach((button) => {
    const active = button.dataset.wifiMode === wifiMode;
    button.classList.toggle("is-active", active);
    button.setAttribute("aria-pressed", active ? "true" : "false");
  });
  if (wifiScanPanel) {
    wifiScanPanel.hidden = wifiMode !== "scan";
  }
  if (wifiManualPanel) {
    wifiManualPanel.hidden = wifiMode !== "manual";
  }
  syncWifiSsidValue();
  if (wifiMode === "scan") {
    syncWifiSecurityFromScan();
  } else if (wifiSecuritySelect && !wifiSecuritySelect.value) {
    wifiSecuritySelect.value = "wpa2";
  }
  syncWifiPasswordRequirement();
}

function openWifiDialog() {
  if (!wifiDialog) {
    return;
  }
  wifiDialog.hidden = false;
  wifiDialog.classList.add("is-open");
  if (wifiDialogBackdrop) {
    wifiDialogBackdrop.hidden = false;
  }
  document.body.classList.add("power-dialog-open");
  setWifiMode(wifiMode);
  wifiDialog.querySelector("[data-wifi-dialog-close]")?.focus();
}

function closeWifiDialog() {
  if (!wifiDialog) {
    return;
  }
  wifiDialog.hidden = true;
  wifiDialog.classList.remove("is-open");
  if (wifiDialogBackdrop) {
    wifiDialogBackdrop.hidden = true;
  }
  document.body.classList.remove("power-dialog-open");
  wifiDialogOpen?.focus();
}

function renderNetworkOptions(networks) {
  if (!wifiNetworkSelect) {
    return;
  }
  wifiNetworkSelect.replaceChildren();
  if (!Array.isArray(networks) || networks.length === 0) {
    const option = document.createElement("option");
    option.value = "";
    option.textContent = "No networks found";
    wifiNetworkSelect.append(option);
    syncWifiSsidValue();
    syncWifiSecurityFromScan();
    return;
  }
  networks.forEach((network) => {
    const option = document.createElement("option");
    option.value = network.ssid || "";
    option.dataset.security = network.security || "";
    option.textContent = `${network.ssid || "Hidden network"} · ${(network.security || "unknown").toUpperCase()} · ${network.signal ?? 0}%`;
    wifiNetworkSelect.append(option);
  });
  syncWifiSsidValue();
  syncWifiSecurityFromScan();
}

async function scanWifiNetworks() {
  if (!wifiScanButton) {
    return;
  }
  wifiScanButton.disabled = true;
  if (wifiScanStatus) {
    wifiScanStatus.textContent = "Scanning...";
  }
  try {
    const cache = await fetchJson("/api/wifi/scan", { method: "POST" });
    renderNetworkOptions(cache.networks);
    if (wifiScanStatus) {
      wifiScanStatus.textContent = "Scan complete.";
    }
  } catch (error) {
    if (wifiScanStatus) {
      wifiScanStatus.textContent = "Scan failed.";
    }
  } finally {
    wifiScanButton.disabled = false;
  }
}

if (wifiDialogOpen && wifiDialog) {
  wifiDialogOpen.addEventListener("click", openWifiDialog);
}

wifiDialogCloseButtons.forEach((button) => {
  button.addEventListener("click", closeWifiDialog);
});

if (wifiDialog) {
  document.addEventListener("keydown", (event) => {
    if (event.key === "Escape" && !wifiDialog.hidden) {
      closeWifiDialog();
    }
  });
}

if (wifiDialogBackdrop) {
  wifiDialogBackdrop.addEventListener("click", closeWifiDialog);
}

wifiModeButtons.forEach((button) => {
  button.addEventListener("click", () => setWifiMode(button.dataset.wifiMode));
});

wifiNetworkSelect?.addEventListener("change", () => {
  syncWifiSsidValue();
  syncWifiSecurityFromScan();
});
wifiManualSsid?.addEventListener("input", syncWifiSsidValue);
wifiSecuritySelect?.addEventListener("change", syncWifiPasswordRequirement);
wifiScanButton?.addEventListener("click", scanWifiNetworks);
wifiConnectForm?.addEventListener("submit", () => {
  syncWifiSsidValue();
  syncWifiPasswordRequirement();
});
setWifiMode(wifiMode);

if (liveFields.length || liveMeters.length || liveSignal || liveLogs || wifiDetails.length || powerButton) {
  refreshLiveState();
  liveRefreshTimer = window.setInterval(refreshLiveState, LIVE_REFRESH_MS);
  document.addEventListener("visibilitychange", () => {
    if (!document.hidden) {
      refreshLiveState();
    }
  });
  window.addEventListener("pagehide", () => {
    if (liveRefreshTimer) {
      window.clearInterval(liveRefreshTimer);
    }
  });
}

const streamPreview = document.querySelector("[data-stream-preview]");

if (streamPreview) {
  const frame = streamPreview.querySelector("[data-stream-frame]");
  const placeholder = streamPreview.querySelector("[data-stream-placeholder]");
  const toggle = streamPreview.querySelector("[data-stream-toggle]");
  const choices = streamPreview.querySelectorAll("[data-stream-choice]");
  const sources = {
    main: streamPreview.dataset.mainSrc || "",
    sub: streamPreview.dataset.subSrc || "",
  };
  let activeStream = streamPreview.dataset.initialStream || "main";
  let playing = true;

  function loadPreviewCache() {
    try {
      const cached = JSON.parse(localStorage.getItem(STREAM_PREVIEW_CACHE_KEY) || "{}");
      if (cached.activeStream === "main" || (cached.activeStream === "sub" && sources.sub)) {
        activeStream = cached.activeStream;
      }
      if (typeof cached.playing === "boolean") {
        playing = cached.playing;
      }
    } catch (error) {
    }
  }

  function savePreviewCache() {
    try {
      localStorage.setItem(
        STREAM_PREVIEW_CACHE_KEY,
        JSON.stringify({ activeStream, playing }),
      );
    } catch (error) {
    }
  }

  function activeSource() {
    return sources[activeStream] || sources.main;
  }

  function syncPreview() {
    choices.forEach((choice) => {
      const selected = choice.dataset.streamChoice === activeStream;
      choice.setAttribute("aria-pressed", selected ? "true" : "false");
    });

    if (toggle) {
      toggle.textContent = playing ? "Stop" : "Start";
      toggle.setAttribute("aria-pressed", playing ? "true" : "false");
    }

    if (placeholder) {
      placeholder.hidden = playing;
    }

    if (!frame) {
      savePreviewCache();
      return;
    }

    if (playing) {
      const source = activeSource();
      if (frame.getAttribute("src") !== source) {
        frame.setAttribute("src", source);
      }
    } else {
      frame.setAttribute("src", "about:blank");
    }

    savePreviewCache();
  }

  choices.forEach((choice) => {
    choice.addEventListener("click", () => {
      if (choice.disabled) {
        return;
      }
      activeStream = choice.dataset.streamChoice || "main";
      playing = true;
      syncPreview();
    });
  });

  if (toggle) {
    toggle.addEventListener("click", () => {
      playing = !playing;
      syncPreview();
    });
  }

  loadPreviewCache();
  syncPreview();
}
