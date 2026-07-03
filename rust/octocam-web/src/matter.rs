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
}
