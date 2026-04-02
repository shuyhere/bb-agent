use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use rand::Rng;
use sha2::{Digest, Sha256};

/// Generate a PKCE verifier + challenge pair.
///
/// * **verifier** – 128 random URL-safe characters
/// * **challenge** – `BASE64URL(SHA256(verifier))` with no padding
pub fn generate_pkce() -> (String, String) {
    let verifier = generate_verifier();
    let challenge = compute_challenge(&verifier);
    (verifier, challenge)
}

fn generate_verifier() -> String {
    const CHARSET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789";
    let mut rng = rand::thread_rng();
    (0..128)
        .map(|_| {
            let idx = rng.gen_range(0..CHARSET.len());
            CHARSET[idx] as char
        })
        .collect()
}

fn compute_challenge(verifier: &str) -> String {
    let hash = Sha256::digest(verifier.as_bytes());
    URL_SAFE_NO_PAD.encode(hash)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn verifier_length_and_charset() {
        let (verifier, _) = generate_pkce();
        assert_eq!(verifier.len(), 128);
        assert!(verifier.chars().all(|c| c.is_ascii_alphanumeric()));
    }

    #[test]
    fn challenge_is_base64url_no_pad() {
        let (_, challenge) = generate_pkce();
        // SHA-256 → 32 bytes → 43 base64url chars (no padding)
        assert_eq!(challenge.len(), 43);
        assert!(!challenge.contains('='));
        assert!(!challenge.contains('+'));
        assert!(!challenge.contains('/'));
    }

    #[test]
    fn deterministic_challenge() {
        let verifier = "test_verifier_string";
        let c1 = compute_challenge(verifier);
        let c2 = compute_challenge(verifier);
        assert_eq!(c1, c2);
    }
}
