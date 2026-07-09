"use strict";

const crypto = require("crypto");
const fs = require("fs");
const path = require("path");
const { spawn } = require("child_process");

const qrcode = require("qrcode");
const {
  Accessory,
  CameraController,
  Categories,
  Characteristic,
  HAPStorage,
  Service,
  uuid,
} = require("hap-nodejs");

const SETTINGS_PATH = process.env.OCTOCAM_CONFIG_PATH || "/var/lib/octocam/settings.json";
const STATE_DIR = process.env.OCTOCAM_STATE_DIR || "/var/lib/octocam";
const STATUS_PATH = process.env.OCTOCAM_HOMEKIT_STATUS_PATH || path.join(STATE_DIR, "homekit-status.json");
const IDENTITY_PATH = process.env.OCTOCAM_HOMEKIT_IDENTITY_PATH || path.join(STATE_DIR, "homekit-identity.json");
const STORAGE_DIR = process.env.OCTOCAM_HOMEKIT_STORAGE_DIR || path.join(STATE_DIR, "hap-storage");
const HAP_PORT = Number.parseInt(process.env.OCTOCAM_HOMEKIT_PORT || "51827", 10);
const DEBUG_FFMPEG = process.env.OCTOCAM_HOMEKIT_FFMPEG_DEBUG === "1";
const MAX_HOMEKIT_WIDTH = Number.parseInt(process.env.OCTOCAM_HOMEKIT_MAX_WIDTH || "640", 10);
const MAX_HOMEKIT_HEIGHT = Number.parseInt(process.env.OCTOCAM_HOMEKIT_MAX_HEIGHT || "480", 10);
const SENSOR_ASPECT_WIDTH = 4;
const SENSOR_ASPECT_HEIGHT = 3;

const FFMPEG_H264_PROFILES = ["baseline", "main", "high"];
const FFMPEG_H264_LEVELS = ["3.1", "3.2", "4.0"];
const SRTP_AES_CM_128_HMAC_SHA1_80 = 0;
const STREAM_REQUEST_START = "start";
const STREAM_REQUEST_STOP = "stop";
const STREAM_REQUEST_RECONFIGURE = "reconfigure";
const usedPorts = new Set();

function ensureDir(dir) {
  fs.mkdirSync(dir, { recursive: true, mode: 0o700 });
}

function readJson(file, fallback) {
  try {
    return JSON.parse(fs.readFileSync(file, "utf8"));
  } catch (_) {
    return fallback;
  }
}

function writeJson(file, data) {
  ensureDir(path.dirname(file));
  fs.writeFileSync(file, `${JSON.stringify(data, null, 2)}\n`, { mode: 0o600 });
}

function loadSettings() {
  return readJson(SETTINGS_PATH, {});
}

function randomPincode() {
  const digits = crypto.randomInt(10000000, 99999999).toString();
  return `${digits.slice(0, 3)}-${digits.slice(3, 5)}-${digits.slice(5)}`;
}

function randomSetupId() {
  return crypto.randomBytes(3).toString("base64").replace(/[^A-Z0-9]/gi, "").slice(0, 4).toUpperCase();
}

function randomUsername() {
  const bytes = crypto.randomBytes(6);
  bytes[0] = (bytes[0] | 0x02) & 0xfe;
  return Array.from(bytes, (byte) => byte.toString(16).padStart(2, "0").toUpperCase()).join(":");
}

function loadIdentity() {
  const existing = readJson(IDENTITY_PATH, null);
  if (existing && existing.username && existing.pincode && existing.setup_id) {
    return existing;
  }
  const identity = {
    username: randomUsername(),
    pincode: randomPincode(),
    setup_id: randomSetupId(),
  };
  writeJson(IDENTITY_PATH, identity);
  return identity;
}

function rtspPath(settings, stream) {
  const pathValue = stream === "sub" && settings.sub_stream_enabled
    ? settings.sub_rtsp_path
    : settings.rtsp_path;
  return String(pathValue || (stream === "sub" ? "sub" : "main")).replace(/^\/+/, "");
}

function rtspUrl(settings, stream) {
  return `rtsp://127.0.0.1:8554/${rtspPath(settings, stream)}`;
}

function sourceStream(settings) {
  // Always capture snapshots from the main stream (native rpiCamera source).
  // Pulling snapshots from the sub stream requires mediamtx to spin up its
  // runOnDemand transcoder (ffmpeg), which takes too long (>6s) on the Pi Zero 2 W
  // and causes HomeKit snapshot timeouts while keeping the sub stream active forever.
  return "main";
}

const MAIN_QUALITY_MIN_HEIGHT = 720;
const MAIN_QUALITY_MIN_BITRATE_KBPS = 500;
const SNAPSHOT_CACHE_TTL_MS = 5000;

function localIpv4Prefixes() {
  const os = require("os");
  const prefixes = [];
  for (const addrs of Object.values(os.networkInterfaces() || {})) {
    for (const addr of addrs || []) {
      if (addr.family === "IPv4" && !addr.internal) {
        prefixes.push(addr.address.split(".").slice(0, 3).join(".") + ".");
      }
    }
  }
  return prefixes;
}

// Local viewers get the main input; remote/cellular (small frame, tight bitrate —
// HomeKit's remote profile) get sub. Network address alone is unreliable because
// hub-relayed remote sessions present LAN addresses, so requested quality is the
// primary signal. This only changes the ffmpeg INPUT — the encoded output stays
// capped by MAX_HOMEKIT_WIDTH/HEIGHT for CPU safety on the Zero 2 W.
function chooseStream(settings, video, targetAddress) {
  if (!settings.sub_stream_enabled) return "main";
  const height = Number.parseInt((video && video.height) || 0, 10);
  const bitrate = Number.parseInt((video && video.max_bit_rate) || 0, 10);
  const wantsMainQuality =
    height >= MAIN_QUALITY_MIN_HEIGHT || bitrate >= MAIN_QUALITY_MIN_BITRATE_KBPS;
  if (!wantsMainQuality) return "sub";
  if (targetAddress && targetAddress.includes(".")) {
    const onLan = localIpv4Prefixes().some((prefix) => targetAddress.startsWith(prefix));
    if (!onLan) return "sub";
  }
  return "main";
}

let snapshotCache = { at: 0, buffer: null };
let snapshotInFlight = null;

function homekitDimensions(width, height) {
  const safeWidth = Math.max(160, Number.parseInt(width || MAX_HOMEKIT_WIDTH, 10));
  const safeHeight = Math.max(120, Number.parseInt(height || MAX_HOMEKIT_HEIGHT, 10));
  const widthLimit = Math.min(safeWidth, MAX_HOMEKIT_WIDTH);
  const heightLimit = Math.min(safeHeight, MAX_HOMEKIT_HEIGHT);
  const unit = Math.max(40, Math.floor(Math.min(widthLimit / SENSOR_ASPECT_WIDTH, heightLimit / SENSOR_ASPECT_HEIGHT)));
  return {
    width: Math.floor(unit * SENSOR_ASPECT_WIDTH / 2) * 2,
    height: Math.floor(unit * SENSOR_ASPECT_HEIGHT / 2) * 2,
  };
}

function supportedResolutions(settings) {
  const subWidth = settings.sub_resolution_width || 640;
  const subHeight = settings.sub_resolution_height || 480;
  const subFps = settings.sub_framerate || 10;
  const primary = homekitDimensions(subWidth, subHeight);
  const candidates = [
    // Advertised so local Home-app sessions request high quality — that request is
    // the local/remote signal chooseStream keys on. The actual ffmpeg OUTPUT stays
    // capped by MAX_HOMEKIT_WIDTH/HEIGHT: switching the INPUT to main improves
    // source quality without betting the Zero 2 W CPU on 720p software encode.
    [1280, 720, Math.min(15, settings.framerate || 15)],
    [primary.width, primary.height, subFps],
    [640, 480, 15],
    [480, 360, 15],
    [320, 240, 15],
  ];
  const seen = new Set();
  return candidates.filter(([width, height, fps]) => {
    const key = `${width}x${height}@${fps}`;
    if (seen.has(key)) {
      return false;
    }
    seen.add(key);
    return true;
  });
}

function nextPort() {
  for (let port = 5100; port < 5300; port += 2) {
    if (!usedPorts.has(port)) {
      usedPorts.add(port);
      return port;
    }
  }
  throw new Error("No free HomeKit RTP ports available");
}

function releasePort(port) {
  if (port) {
    usedPorts.delete(port);
  }
}

function runProcess(command, args, timeoutMs, stdin) {
  return new Promise((resolve, reject) => {
    const child = spawn(command, args, { env: process.env, stdio: ["pipe", "pipe", "pipe"] });
    const stdout = [];
    const stderr = [];
    const timer = setTimeout(() => {
      child.kill("SIGKILL");
      reject(new Error(`${command} timed out`));
    }, timeoutMs);

    child.stdout.on("data", (chunk) => stdout.push(chunk));
    child.stderr.on("data", (chunk) => stderr.push(chunk));
    child.on("error", (error) => {
      clearTimeout(timer);
      reject(error);
    });
    child.on("exit", (code, signal) => {
      clearTimeout(timer);
      if (code === 0 && !signal) {
        resolve(Buffer.concat(stdout));
      } else {
        reject(new Error(Buffer.concat(stderr).toString("utf8").trim() || `${command} exited with ${code || signal}`));
      }
    });

    if (stdin) {
      child.stdin.end(stdin);
    } else {
      child.stdin.end();
    }
  });
}

class OctoCamStreamingDelegate {
  constructor() {
    this.pendingSessions = new Map();
    this.ongoingSessions = new Map();
    this.controller = null;
  }

  handleSnapshotRequest(request, callback) {
    const now = Date.now();
    if (snapshotCache.buffer && now - snapshotCache.at < SNAPSHOT_CACHE_TTL_MS) {
      callback(undefined, snapshotCache.buffer);
      return;
    }
    if (snapshotInFlight) {
      snapshotInFlight.then((buffer) => callback(undefined, buffer)).catch((error) => callback(error));
      return;
    }

    const settings = loadSettings();
    const stream = sourceStream(settings);
    const dimensions = homekitDimensions(request.width, request.height);
    console.log(`HomeKit snapshot request: requested=${request.width}x${request.height} actual=${dimensions.width}x${dimensions.height} using ${stream}`);
    const args = [
      "-hide_banner",
      "-loglevel", "error",
      "-rtsp_transport", "tcp",
      "-fflags", "nobuffer",
      "-flags", "low_delay",
      "-i", rtspUrl(settings, stream),
      "-frames:v", "1",
      "-vf", `scale=${dimensions.width}:${dimensions.height}:force_original_aspect_ratio=decrease`,
      "-f", "image2pipe",
      "-vcodec", "mjpeg",
      "-",
    ];

    snapshotInFlight = runProcess("ffmpeg", args, 6000)
      .then((buffer) => {
        console.log(`HomeKit snapshot captured: ${buffer.length} bytes`);
        snapshotCache = { at: Date.now(), buffer };
        snapshotInFlight = null;
        return buffer;
      })
      .catch((error) => {
        console.log(`HomeKit snapshot failed: ${error.message}`);
        snapshotInFlight = null;
        throw error;
      });
    snapshotInFlight.then((buffer) => callback(undefined, buffer)).catch((error) => callback(error));
  }

  prepareStream(request, callback) {
    const sessionId = request.sessionID;
    const video = request.video;
    const localVideoPort = nextPort();
    const videoSsrc = CameraController.generateSynchronisationSource();

    this.pendingSessions.set(sessionId, {
      address: request.targetAddress,
      localVideoPort,
      videoPort: video.port,
      videoCryptoSuite: video.srtpCryptoSuite,
      videoSrtp: Buffer.concat([video.srtp_key, video.srtp_salt]),
      videoSsrc,
    });

    console.log(`HomeKit prepare stream: target=${request.targetAddress}:${video.port} local=${localVideoPort} suite=${video.srtpCryptoSuite}`);
    callback(undefined, {
      video: {
        port: localVideoPort,
        ssrc: videoSsrc,
        srtp_key: video.srtp_key,
        srtp_salt: video.srtp_salt,
      },
    });
  }

  handleStreamRequest(request, callback) {
    switch (request.type) {
      case STREAM_REQUEST_START:
        this.startStream(request, callback);
        break;
      case STREAM_REQUEST_RECONFIGURE:
        callback();
        break;
      case STREAM_REQUEST_STOP:
        this.stopStream(request.sessionID);
        callback();
        break;
      default:
        callback();
        break;
    }
  }

  buildStreamArgs(settings, sessionInfo, video, stream) {
    const dimensions = homekitDimensions(video.width, video.height);
    const mtu = video.mtu || 1316;
    const profile = FFMPEG_H264_PROFILES[video.profile] || "baseline";
    const level = FFMPEG_H264_LEVELS[video.level] || "3.1";
    const maxBitrate = Math.max(128, video.max_bit_rate || settings.sub_bitrate_kbps || 600);
    const fps = Math.max(1, Math.min(30, video.fps || settings.sub_framerate || 10));
    const srtpParams = sessionInfo.videoSrtp.toString("base64");

    const args = [
      "-hide_banner",
      "-nostdin",
      "-rtsp_transport", "tcp",
      "-fflags", "nobuffer",
      "-flags", "low_delay",
      "-i", rtspUrl(settings, stream),
      "-map", "0:v:0",
      "-an",
      "-sn",
      "-dn",
      "-vf", `scale=${dimensions.width}:${dimensions.height}`,
      "-c:v", "libx264",
      "-preset", "ultrafast",
      "-tune", "zerolatency",
      "-pix_fmt", "yuv420p",
      "-r", String(fps),
      "-b:v", `${maxBitrate}k`,
      "-maxrate", `${maxBitrate}k`,
      "-bufsize", `${Math.max(maxBitrate, 128)}k`,
      "-g", String(Math.max(10, fps * 2)),
      "-bf", "0",
      "-profile:v", profile,
      "-level:v", level,
      "-payload_type", String(video.pt),
      "-ssrc", String(sessionInfo.videoSsrc),
      "-f", "rtp",
      "-srtp_out_suite", "AES_CM_128_HMAC_SHA1_80",
      "-srtp_out_params", srtpParams,
      `srtp://${sessionInfo.address}:${sessionInfo.videoPort}?rtcpport=${sessionInfo.videoPort}&localrtcpport=${sessionInfo.localVideoPort}&pkt_size=${mtu}`,
    ];

    return { args, dimensions, fps, maxBitrate };
  }

  startStream(request, callback) {
    const sessionInfo = this.pendingSessions.get(request.sessionID);
    if (!sessionInfo) {
      callback(new Error("HomeKit session was not prepared"));
      return;
    }

    const settings = loadSettings();
    const stream = chooseStream(settings, request.video, sessionInfo.address);
    const video = request.video;

    let callbackCalled = false;
    const finishStart = (error) => {
      if (!callbackCalled) {
        callbackCalled = true;
        callback(error);
      }
    };

    const attempt = (streamName, allowSubFallback) => {
      const { args, dimensions, fps, maxBitrate } = this.buildStreamArgs(settings, sessionInfo, video, streamName);

      console.log(`Starting HomeKit ${streamName} stream: requested=${video.width}x${video.height} actual=${dimensions.width}x${dimensions.height} ${fps}fps ${maxBitrate}kbps`);
      if (DEBUG_FFMPEG) {
        console.log(`ffmpeg ${args.join(" ")}`);
      }

      const child = spawn("ffmpeg", args, { env: process.env, stdio: ["ignore", "ignore", "pipe"] });
      const stderrLines = [];
      let retriedWithSub = false;
      // Only before the stream is confirmed started (finishStart not yet fired):
      // fall back from main to sub exactly once, reusing the same session/port.
      const retryWithSub = (reason) => {
        if (retriedWithSub || !allowSubFallback || callbackCalled || !settings.sub_stream_enabled) {
          return false;
        }
        retriedWithSub = true;
        console.log(`HomeKit main stream failed to start (${reason}); retrying with sub`);
        attempt("sub", false);
        return true;
      };
      const startedTimer = setTimeout(() => {
        finishStart();
      }, 700);

      child.stderr.on("data", (chunk) => {
        const lines = chunk.toString("utf8").trim().split("\n").filter(Boolean);
        stderrLines.push(...lines);
        if (stderrLines.length > 20) {
          stderrLines.splice(0, stderrLines.length - 20);
        }
        if (DEBUG_FFMPEG) {
          console.log(lines.join("\n"));
        }
      });
      child.on("error", (error) => {
        clearTimeout(startedTimer);
        console.log(`HomeKit ffmpeg failed to start: ${error.message}`);
        if (retryWithSub(error.message)) {
          return;
        }
        finishStart(error);
      });
      child.on("exit", (code, signal) => {
        clearTimeout(startedTimer);
        const failed = code !== 0 && code !== 255 && signal !== "SIGKILL";
        if (failed && retryWithSub(`exit code=${code} signal=${signal}: ${stderrLines.join(" | ")}`)) {
          // The retry attempt reuses this session's port and replaced the
          // ongoingSessions entry — leave bookkeeping to it.
          return;
        }
        releasePort(sessionInfo.localVideoPort);
        this.ongoingSessions.delete(request.sessionID);
        if (failed && this.controller) {
          console.log(`HomeKit ffmpeg stream exited with code=${code} signal=${signal}: ${stderrLines.join(" | ")}`);
          finishStart(new Error(`ffmpeg exited with ${code || signal}`));
          this.controller.forceStopStreamingSession(request.sessionID);
        } else {
          console.log(`HomeKit ffmpeg stream stopped with code=${code} signal=${signal}`);
        }
      });

      this.ongoingSessions.set(request.sessionID, {
        process: child,
        localVideoPort: sessionInfo.localVideoPort,
      });
    };

    attempt(stream, stream === "main");
    this.pendingSessions.delete(request.sessionID);
  }

  stopStream(sessionId) {
    console.log(`Stopping HomeKit stream: ${sessionId}`);
    const session = this.ongoingSessions.get(sessionId);
    if (!session) {
      const pending = this.pendingSessions.get(sessionId);
      if (pending) {
        releasePort(pending.localVideoPort);
        this.pendingSessions.delete(sessionId);
      }
      return;
    }
    try {
      session.process.kill("SIGKILL");
    } catch (_) {
    }
    releasePort(session.localVideoPort);
    this.ongoingSessions.delete(sessionId);
  }
}

async function writeStatus(accessory, identity, extra = {}) {
  let setupUri = "";
  try {
    setupUri = accessory.setupURI();
  } catch (_) {
  }
  const qrDataUrl = setupUri ? await qrcode.toDataURL(setupUri, { margin: 1, width: 320 }) : "";
  writeJson(STATUS_PATH, {
    status: extra.status || "ready",
    paired: Boolean(extra.paired),
    pincode: identity.pincode,
    setup_id: identity.setup_id,
    setup_uri: setupUri,
    qr_data_url: qrDataUrl,
    updated_at: new Date().toISOString(),
  });
}

async function main() {
  ensureDir(STATE_DIR);
  ensureDir(STORAGE_DIR);
  HAPStorage.setCustomStoragePath(STORAGE_DIR);

  const settings = loadSettings();
  const identity = loadIdentity();
  const displayName = settings.camera_label || settings.device_name || "OctoCam";
  const accessory = new Accessory(displayName, uuid.generate(`octocam:camera:${identity.username}`));
  const delegate = new OctoCamStreamingDelegate();
  const cameraController = new CameraController({
    cameraStreamCount: 2,
    delegate,
    streamingOptions: {
      supportedCryptoSuites: [SRTP_AES_CM_128_HMAC_SHA1_80],
      video: {
        codec: {
          profiles: [0, 1, 2],
          levels: [0, 1, 2],
        },
        resolutions: supportedResolutions(settings),
      },
    },
  });
  delegate.controller = cameraController;

  accessory
    .getService(Service.AccessoryInformation)
    .setCharacteristic(Characteristic.Manufacturer, "OctoCam")
    .setCharacteristic(Characteristic.Model, "Raspberry Pi Zero 2 W Camera")
    .setCharacteristic(Characteristic.Name, displayName)
    .setCharacteristic(Characteristic.SerialNumber, identity.username.replace(/:/g, ""));

  accessory.on("identify", (_paired, callback) => callback());
  accessory.on("paired", () => writeStatus(accessory, identity, { status: "paired", paired: true }).catch(console.error));
  accessory.on("unpaired", () => writeStatus(accessory, identity, { status: "ready", paired: false }).catch(console.error));
  accessory.configureController(cameraController);

  await accessory.publish({
    port: HAP_PORT,
    username: identity.username,
    pincode: identity.pincode,
    setupID: identity.setup_id,
    category: Categories.IP_CAMERA || Categories.CAMERA,
  });

  const paired = Boolean(accessory._accessoryInfo && accessory._accessoryInfo.paired());
  await writeStatus(accessory, identity, { status: paired ? "paired" : "ready", paired });
  console.log(`OctoCam HomeKit camera published as ${displayName} on port ${HAP_PORT}`);

  const shutdown = async () => {
    try {
      await accessory.unpublish();
    } finally {
      process.exit(0);
    }
  };
  process.on("SIGINT", shutdown);
  process.on("SIGTERM", shutdown);
}

main().catch((error) => {
  console.error(error);
  writeJson(STATUS_PATH, {
    status: "error",
    paired: false,
    error: error.message,
    updated_at: new Date().toISOString(),
  });
  process.exit(1);
});
