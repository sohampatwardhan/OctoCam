use rand::Rng;
use serde::{Deserialize, Serialize};
use std::{fs, io, path::Path, path::PathBuf};

pub const VENDOR_ID: u16 = 0xFFF1; // CSA test VID; not shippable as a product
pub const PRODUCT_ID: u16 = 0x8001;

/// Matter spec 5.1.7.1: these passcodes are invalid and must never be used.
const INVALID_PASSCODES: [u32; 12] = [
    0, 11111111, 22222222, 33333333, 44444444, 55555555, 66666666, 77777777,
    88888888, 99999999, 12345678, 87654321,
];

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct MatterIdentity {
    pub passcode: u32,      // 27-bit, 1..=99999998, excluding INVALID_PASSCODES
    pub discriminator: u16, // 12-bit
    pub vendor_id: u16,
    pub product_id: u16,
}

pub fn generate_identity() -> MatterIdentity {
    let mut rng = rand::thread_rng();
    let passcode = loop {
        let candidate = rng.gen_range(1..=99999998u32);
        if !INVALID_PASSCODES.contains(&candidate) {
            break candidate;
        }
    };
    MatterIdentity {
        passcode,
        discriminator: rng.gen_range(0..=4095u16),
        vendor_id: VENDOR_ID,
        product_id: PRODUCT_ID,
    }
}

/// Load the persisted identity, or generate + persist one (file mode 0600 —
/// the passcode is a durable commission-this-camera credential; see the spec's
/// documented deviation from the no-plaintext-secrets model).
pub fn load_or_generate_identity(path: &Path) -> io::Result<MatterIdentity> {
    if let Ok(raw) = fs::read_to_string(path) {
        if let Ok(identity) = serde_json::from_str::<MatterIdentity>(&raw) {
            return Ok(identity);
        }
    }
    let identity = generate_identity();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    #[cfg(unix)]
    {
        use std::io::Write;
        use std::os::unix::fs::OpenOptionsExt;
        // Remove any pre-existing (e.g. corrupt) file so `.mode(0o600)` always
        // applies — mode only takes effect when the open creates the file.
        let _ = fs::remove_file(path);
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(path)?;
        file.write_all(serde_json::to_string_pretty(&identity)?.as_bytes())?;
    }
    #[cfg(not(unix))]
    fs::write(path, serde_json::to_string_pretty(&identity)?)?;
    Ok(identity)
}

/// Reset pairing rotates the passcode: delete and regenerate.
pub fn rotate_identity(path: &Path) -> io::Result<MatterIdentity> {
    let _ = fs::remove_file(path);
    load_or_generate_identity(path)
}

const BASE38_CHARS: &[u8] = b"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZ-.";

/// Pack the onboarding payload bit fields LSB-first into 11 bytes
/// (Matter spec 5.1.3): version(3)=0, vid(16), pid(16), custom-flow(2)=0,
/// discovery-capabilities(8)=0x04 (on-network only), discriminator(12),
/// passcode(27), padding(4)=0.
fn pack_payload_bits(id: &MatterIdentity) -> [u8; 11] {
    let mut acc: u128 = 0;
    let mut shift = 0u32;
    for (value, width) in [
        (0u128, 3u32),
        (u128::from(id.vendor_id), 16),
        (u128::from(id.product_id), 16),
        (0, 2),
        (0x04, 8),
        (u128::from(id.discriminator & 0x0FFF), 12),
        (u128::from(id.passcode & 0x07FF_FFFF), 27),
        (0, 4),
    ] {
        acc |= value << shift;
        shift += width;
    }
    let mut bytes = [0u8; 11];
    for (i, byte) in bytes.iter_mut().enumerate() {
        *byte = ((acc >> (8 * i)) & 0xFF) as u8;
    }
    bytes
}

/// Base38 per Matter spec 5.1.3.1: bytes consumed in groups of 3 (LE u32 → 5
/// chars), a trailing 2-byte group → 4 chars, 1-byte → 2 chars.
fn base38_encode(bytes: &[u8]) -> String {
    let mut out = String::new();
    for chunk in bytes.chunks(3) {
        let (mut value, chars) = match chunk.len() {
            3 => (u32::from(chunk[0]) | u32::from(chunk[1]) << 8 | u32::from(chunk[2]) << 16, 5),
            2 => (u32::from(chunk[0]) | u32::from(chunk[1]) << 8, 4),
            _ => (u32::from(chunk[0]), 2),
        };
        for _ in 0..chars {
            out.push(BASE38_CHARS[(value % 38) as usize] as char);
            value /= 38;
        }
    }
    out
}

#[cfg(test)]
fn base38_decode(text: &str) -> [u8; 11] {
    let mut bytes = Vec::new();
    let chars: Vec<u32> = text
        .bytes()
        .map(|b| BASE38_CHARS.iter().position(|c| *c == b).unwrap() as u32)
        .collect();
    for group in chars.chunks(5) {
        let value = group.iter().rev().fold(0u32, |acc, c| acc * 38 + c);
        let n = match group.len() {
            5 => 3,
            4 => 2,
            _ => 1,
        };
        for i in 0..n {
            bytes.push(((value >> (8 * i)) & 0xFF) as u8);
        }
    }
    bytes.try_into().unwrap()
}

pub fn qr_payload(id: &MatterIdentity) -> String {
    format!("MT:{}", base38_encode(&pack_payload_bits(id)))
}

/// 11-digit manual pairing code (Matter spec 5.1.4.1, VID/PID not included):
/// digit1 = short-discriminator(4 bits) >> 2; next 5 digits =
/// ((short_disc & 3) << 14) | (passcode & 0x3FFF); next 4 = passcode >> 14;
/// final digit = Verhoeff checksum over the first 10.
pub fn manual_pairing_code(discriminator: u16, passcode: u32) -> String {
    let short_disc = u32::from((discriminator >> 8) & 0xF);
    let digit1 = short_disc >> 2; // VID_PID_PRESENT = 0
    let chunk2 = ((short_disc & 0x3) << 14) | (passcode & 0x3FFF);
    let chunk3 = passcode >> 14;
    let first10 = format!("{digit1}{chunk2:05}{chunk3:04}");
    format!("{first10}{}", verhoeff_digit(&first10))
}

fn verhoeff_digit(digits: &str) -> u32 {
    const D: [[u8; 10]; 10] = [
        [0, 1, 2, 3, 4, 5, 6, 7, 8, 9],
        [1, 2, 3, 4, 0, 6, 7, 8, 9, 5],
        [2, 3, 4, 0, 1, 7, 8, 9, 5, 6],
        [3, 4, 0, 1, 2, 8, 9, 5, 6, 7],
        [4, 0, 1, 2, 3, 9, 5, 6, 7, 8],
        [5, 9, 8, 7, 6, 0, 4, 3, 2, 1],
        [6, 5, 9, 8, 7, 1, 0, 4, 3, 2],
        [7, 6, 5, 9, 8, 2, 1, 0, 4, 3],
        [8, 7, 6, 5, 9, 3, 2, 1, 0, 4],
        [9, 8, 7, 6, 5, 4, 3, 2, 1, 0],
    ];
    const P: [[u8; 10]; 8] = [
        [0, 1, 2, 3, 4, 5, 6, 7, 8, 9],
        [1, 5, 7, 6, 2, 8, 3, 0, 9, 4],
        [5, 8, 0, 3, 7, 9, 6, 1, 4, 2],
        [8, 9, 1, 6, 0, 4, 3, 5, 2, 7],
        [9, 4, 5, 3, 1, 2, 6, 8, 7, 0],
        [4, 2, 8, 6, 5, 7, 3, 9, 0, 1],
        [2, 7, 9, 3, 8, 0, 6, 4, 1, 5],
        [7, 0, 4, 6, 9, 1, 3, 2, 5, 8],
    ];
    const INV: [u8; 10] = [0, 4, 3, 2, 1, 5, 6, 7, 8, 9];
    let mut c = 0u8;
    for (i, ch) in digits.bytes().rev().enumerate() {
        let digit = ch - b'0';
        c = D[c as usize][P[(i + 1) % 8][digit as usize] as usize];
    }
    u32::from(INV[c as usize])
}

pub fn qr_svg(payload: &str) -> String {
    use qrcode::render::svg;
    match qrcode::QrCode::new(payload.as_bytes()) {
        Ok(code) => {
            let rendered = code
                .render::<svg::Color>()
                .min_dimensions(180, 180)
                .dark_color(svg::Color("#000000"))
                .light_color(svg::Color("#ffffff"))
                .build();
            // The renderer prepends an XML declaration; strip it so the
            // result embeds directly into HTML.
            match rendered.find("<svg") {
                Some(start) => rendered[start..].to_string(),
                None => rendered,
            }
        }
        Err(_) => String::new(),
    }
}

pub fn default_identity_path() -> PathBuf {
    std::env::var_os("OCTOCAM_MATTER_IDENTITY_PATH")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/var/lib/octocam/matter-identity.json"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generated_identity_is_in_spec_ranges() {
        for _ in 0..200 {
            let id = generate_identity();
            assert!((1..=99999998).contains(&id.passcode));
            assert!(!INVALID_PASSCODES.contains(&id.passcode));
            assert!(id.discriminator <= 4095);
            assert_eq!(id.vendor_id, 0xFFF1);
            assert_eq!(id.product_id, 0x8001);
        }
    }

    #[test]
    fn identity_persists_and_rotates() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("matter-identity.json");
        let first = load_or_generate_identity(&path).unwrap();
        let again = load_or_generate_identity(&path).unwrap();
        assert_eq!(first, again, "identity must be stable across loads");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
            assert_eq!(mode, 0o600);
        }
        let rotated = rotate_identity(&path).unwrap();
        assert_ne!(first.passcode, rotated.passcode, "reset must rotate the passcode");
    }

    /// Known CHIP test vector: discriminator 3840, passcode 20202021.
    /// Digits derived: digit1=3 (short disc 15 >> 2), chunk2=49701, chunk3=1233,
    /// Verhoeff check digit 2 → 34970112332 (matches chip-tool's documented code).
    #[test]
    fn manual_pairing_code_matches_chip_test_vector() {
        assert_eq!(manual_pairing_code(3840, 20202021), "34970112332");
    }

    #[test]
    fn qr_payload_shape_and_roundtrip() {
        let id = MatterIdentity {
            passcode: 20202021,
            discriminator: 3840,
            vendor_id: 0xFFF1,
            product_id: 0x8001,
        };
        let payload = qr_payload(&id);
        assert!(payload.starts_with("MT:"));
        assert_eq!(payload.len(), 3 + 19, "88 bits → 11 bytes → 19 base38 chars");
        let bytes = pack_payload_bits(&id);
        let decoded = base38_decode(&payload[3..]);
        assert_eq!(decoded, bytes, "base38 must round-trip");
        // Field-level checks against the packed bits (LSB-first layout).
        let acc = bytes.iter().rev().fold(0u128, |acc, b| (acc << 8) | u128::from(*b));
        assert_eq!(acc & 0x7, 0, "version");
        assert_eq!((acc >> 3) & 0xFFFF, 0xFFF1, "vid");
        assert_eq!((acc >> 19) & 0xFFFF, 0x8001, "pid");
        assert_eq!((acc >> 35) & 0x3, 0, "custom flow");
        assert_eq!((acc >> 37) & 0xFF, 0x04, "discovery: on-network");
        assert_eq!((acc >> 45) & 0xFFF, 3840, "discriminator");
        assert_eq!((acc >> 57) & 0x7FF_FFFF, 20202021, "passcode");
    }

    #[test]
    fn qr_svg_renders() {
        let id = generate_identity();
        let svg = qr_svg(&qr_payload(&id));
        assert!(svg.starts_with("<svg"));
        assert!(svg.contains("</svg>"));
    }
}
