use base64::{
    engine::general_purpose::{STANDARD, STANDARD_NO_PAD, URL_SAFE, URL_SAFE_NO_PAD},
    Engine,
};
use constant_time_eq::constant_time_eq;
use hmac::{Hmac, Mac};
use pbkdf2::pbkdf2_hmac;
use rand::RngCore;
use sha2::Sha256;

pub fn decode_base64_url(input: &str) -> Result<Vec<u8>, base64::DecodeError> {
    let trimmed = input.trim();
    URL_SAFE_NO_PAD
        .decode(trimmed)
        .or_else(|_| URL_SAFE.decode(trimmed))
        .or_else(|_| STANDARD_NO_PAD.decode(trimmed))
        .or_else(|_| STANDARD.decode(trimmed))
}

pub fn encode_base64_url(bytes: &[u8]) -> String {
    URL_SAFE_NO_PAD.encode(bytes)
}

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

#[allow(dead_code)]
pub fn sign_session(secret: &str) -> String {
    sign_session_for_user(secret, 1, "admin")
}

pub fn sign_session_for_user(secret: &str, user_id: i64, username: &str) -> String {
    let payload = format!("{user_id}:{username}");
    let sig = signature(secret, &payload);
    format!("{payload}.{sig}")
}

#[allow(dead_code)]
pub fn verify_session(secret: &str, cookie_value: &str) -> bool {
    verify_session_for_user(secret, cookie_value).is_some()
}

pub fn verify_session_for_user(secret: &str, cookie_value: &str) -> Option<(i64, String)> {
    let Some((payload, signature_value)) = cookie_value.split_once('.') else {
        return None;
    };
    if payload == SESSION_VALUE {
        let expected_sig = signature(secret, SESSION_VALUE);
        if constant_time_eq(expected_sig.as_bytes(), signature_value.as_bytes()) {
            return Some((1, "admin".to_string()));
        }
        return None;
    }
    let expected_sig = signature(secret, payload);
    if !constant_time_eq(expected_sig.as_bytes(), signature_value.as_bytes()) {
        return None;
    }
    let (user_id_str, username) = payload.split_once(':')?;
    let user_id = user_id_str.parse::<i64>().ok()?;
    Some((user_id, username.to_string()))
}

pub fn generate_random_bytes(len: usize) -> Vec<u8> {
    let mut buf = vec![0_u8; len];
    rand::thread_rng().fill_bytes(&mut buf);
    buf
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
