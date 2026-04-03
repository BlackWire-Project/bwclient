use anyhow::{Context, Result, anyhow, bail};
use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use chacha20poly1305::{
    ChaCha20Poly1305, KeyInit,
    aead::{Aead, Payload},
};
use ed25519_dalek::{SigningKey, VerifyingKey};
use hkdf::Hkdf;
use rand::{RngCore, rngs::OsRng};
use serde::Deserialize;
use sha2::Sha256;
use x25519_dalek::{PublicKey as X25519PublicKey, StaticSecret};

use super::types::EncryptedPayload;

pub(crate) fn signing_key_from_b64(value: &str) -> Result<SigningKey> {
    Ok(SigningKey::from_bytes(&decode_32(value)?))
}

pub(crate) fn verifying_key_from_b64(value: &str) -> Result<VerifyingKey> {
    Ok(VerifyingKey::from_bytes(&decode_32(value)?)?)
}

pub(crate) fn static_secret_from_b64(value: &str) -> Result<StaticSecret> {
    Ok(StaticSecret::from(decode_32(value)?))
}

pub(crate) fn x25519_public_from_b64(value: &str) -> Result<X25519PublicKey> {
    Ok(X25519PublicKey::from(decode_32(value)?))
}

pub(crate) fn decode_32(value: &str) -> Result<[u8; 32]> {
    let bytes = decode_bytes(value)?;
    bytes
        .try_into()
        .map_err(|_| anyhow!("expected 32-byte key"))
}

pub fn encode_bytes(value: &[u8]) -> String {
    URL_SAFE_NO_PAD.encode(value)
}

pub fn decode_bytes(value: &str) -> Result<Vec<u8>> {
    URL_SAFE_NO_PAD
        .decode(value.as_bytes())
        .with_context(|| format!("failed to decode base64url payload"))
}

pub(crate) fn encrypt_text(key: &[u8], aad: &str, payload: &EncryptedPayload) -> Result<String> {
    let cipher = ChaCha20Poly1305::new_from_slice(key)?;
    let plaintext = serde_json::to_vec(payload)?;
    let mut nonce = [0u8; 12];
    OsRng.fill_bytes(&mut nonce);
    let ciphertext = cipher
        .encrypt(
            (&nonce).into(),
            Payload {
                msg: &plaintext,
                aad: aad.as_bytes(),
            },
        )
        .map_err(|_| anyhow!("failed to encrypt payload"))?;
    let mut output = nonce.to_vec();
    output.extend_from_slice(&ciphertext);
    Ok(encode_bytes(&output))
}

pub(crate) fn decrypt_text<T: for<'de> Deserialize<'de>>(
    key: &[u8],
    aad: &str,
    ciphertext: &str,
) -> Result<T> {
    let cipher = ChaCha20Poly1305::new_from_slice(key)?;
    let payload = decode_bytes(ciphertext)?;
    if payload.len() < 13 {
        bail!("ciphertext payload too short");
    }
    let (nonce, encrypted) = payload.split_at(12);
    let plaintext = cipher
        .decrypt(
            nonce.into(),
            Payload {
                msg: encrypted,
                aad: aad.as_bytes(),
            },
        )
        .map_err(|_| anyhow!("failed to decrypt payload"))?;
    Ok(serde_json::from_slice(&plaintext)?)
}

pub(crate) fn derive_bytes(input: &[u8], info: &[u8]) -> Result<Vec<u8>> {
    let hk = Hkdf::<Sha256>::new(None, input);
    let mut out = vec![0u8; 32];
    hk.expand(info, &mut out)
        .map_err(|_| anyhow!("hkdf derive failed"))?;
    Ok(out)
}

pub(crate) fn kdf_root(root_key: &[u8], dh_out: &[u8]) -> Result<(Vec<u8>, Vec<u8>)> {
    let hk = Hkdf::<Sha256>::new(Some(root_key), dh_out);
    let mut out = [0u8; 64];
    hk.expand(super::constants::INFO_ROOT, &mut out)
        .map_err(|_| anyhow!("hkdf root derive failed"))?;
    Ok((out[..32].to_vec(), out[32..].to_vec()))
}

pub(crate) fn kdf_chain(chain_key: &[u8]) -> Result<(Vec<u8>, Vec<u8>)> {
    let hk = Hkdf::<Sha256>::new(Some(chain_key), b"");
    let mut out = [0u8; 64];
    hk.expand(super::constants::INFO_CHAIN, &mut out)
        .map_err(|_| anyhow!("hkdf chain derive failed"))?;
    Ok((out[..32].to_vec(), out[32..].to_vec()))
}
