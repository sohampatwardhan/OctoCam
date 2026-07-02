use base64::{engine::general_purpose::URL_SAFE, Engine};
use constant_time_eq::constant_time_eq;
use hmac::{Hmac, Mac};
use pbkdf2::pbkdf2_hmac;
use rand::RngCore;
use sha2::Sha256;

const ALGORITHM: &str = "pbkdf2_sha256";
const ITERATIONS: u32 = 260_000;
const SALT_BYTES: usize = 16;
const SESSION_VALUE: &str = "authenticated";

type HmacSha256 = Hmac<Sha256>;

pub fn hash_password(password: &str) -> String {
    let mut salt = [0_u8; SALT_BYTES];
    rand::thread_rng().fill_bytes(&mut salt);
    let mut digest = [0_u8; 32];
    pbkdf2_hmac::<Sha256>(password.as_bytes(), &salt, ITERATIONS, &mut digest);
    format!(
        "{ALGORITHM}${ITERATIONS}${}${}",
        URL_SAFE.encode(salt),
        URL_SAFE.encode(digest)
    )
}

pub fn verify_password(password: &str, encoded: &str) -> bool {
    let parts: Vec<&str> = encoded.splitn(4, '$').collect();
    if parts.len() != 4 || parts[0] != ALGORITHM {
        return false;
    }
    let Ok(iterations) = parts[1].parse::<u32>() else {
        return false;
    };
    if iterations < 1 {
        return false;
    }
    let Ok(salt) = URL_SAFE.decode(parts[2]) else {
        return false;
    };
    let Ok(expected) = URL_SAFE.decode(parts[3]) else {
        return false;
    };
    let mut actual = vec![0_u8; expected.len()];
    pbkdf2_hmac::<Sha256>(password.as_bytes(), &salt, iterations, &mut actual);
    constant_time_eq(&actual, &expected)
}

pub fn sign_session(secret: &str) -> String {
    let signature = signature(secret, SESSION_VALUE);
    format!("{SESSION_VALUE}.{signature}")
}

pub fn verify_session(secret: &str, cookie_value: &str) -> bool {
    let Some((value, signature_value)) = cookie_value.split_once('.') else {
        return false;
    };
    if value != SESSION_VALUE {
        return false;
    }
    constant_time_eq(
        signature(secret, value).as_bytes(),
        signature_value.as_bytes(),
    )
}

fn signature(secret: &str, value: &str) -> String {
    let mut mac =
        HmacSha256::new_from_slice(secret.as_bytes()).expect("HMAC accepts any key length");
    mac.update(value.as_bytes());
    URL_SAFE.encode(mac.finalize().into_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hashes_and_verifies_passwords() {
        let encoded = hash_password("correct horse battery staple");
        assert!(verify_password("correct horse battery staple", &encoded));
        assert!(!verify_password("wrong password", &encoded));
    }

    #[test]
    fn verifies_existing_pbkdf2_hash() {
        let encoded = "pbkdf2_sha256$1$c2FsdDEyMzQ=$Ze1gBkrzGB_4uQatUMmRG9aOh4jpbYJspXhCDyhe24A=";
        assert!(verify_password("octocam-password", encoded));
    }

    #[test]
    fn signs_and_verifies_session_cookie() {
        let cookie = sign_session("secret");
        assert!(verify_session("secret", &cookie));
        assert!(!verify_session("other", &cookie));
    }
}
