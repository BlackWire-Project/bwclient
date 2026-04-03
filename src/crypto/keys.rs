use anyhow::{Context, Result};
use ed25519_dalek::{Signer, SigningKey};
use rand::{RngCore, rngs::OsRng};
use x25519_dalek::{PublicKey as X25519PublicKey, StaticSecret};

use crate::state::{StoredOneTimePrekey, StoredProfileKeys};

use super::{
    primitives::encode_bytes,
    types::IdentityBundlePublic,
};

pub fn now_iso() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};

    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    format!("{}", now.as_secs())
}

pub fn generate_profile_material(one_time_prekeys: usize) -> Result<(StoredProfileKeys, String)> {
    let mut rng = OsRng;
    let identity_sign = SigningKey::generate(&mut rng);
    let identity_dh = StaticSecret::random_from_rng(&mut rng);
    let signed_prekey = StaticSecret::random_from_rng(&mut rng);
    let signed_prekey_public = X25519PublicKey::from(&signed_prekey);
    let signature = identity_sign.sign(signed_prekey_public.as_bytes());
    let mut one_time = Vec::with_capacity(one_time_prekeys);

    for _ in 0..one_time_prekeys {
        let secret = StaticSecret::random_from_rng(&mut rng);
        let public = X25519PublicKey::from(&secret);
        one_time.push(StoredOneTimePrekey {
            public_key: encode_bytes(public.as_bytes()),
            private_key: encode_bytes(secret.to_bytes().as_slice()),
            uploaded: true,
            consumed: false,
        });
    }

    let keys = StoredProfileKeys {
        identity_sign_secret: encode_bytes(identity_sign.to_bytes().as_slice()),
        identity_sign_public: encode_bytes(identity_sign.verifying_key().as_bytes()),
        identity_dh_secret: encode_bytes(identity_dh.to_bytes().as_slice()),
        identity_dh_public: encode_bytes(X25519PublicKey::from(&identity_dh).as_bytes()),
        signed_prekey_secret: encode_bytes(signed_prekey.to_bytes().as_slice()),
        signed_prekey_public: encode_bytes(signed_prekey_public.as_bytes()),
        signed_prekey_signature: encode_bytes(signature.to_bytes().as_slice()),
        one_time_prekeys: one_time,
    };

    Ok((keys, random_inbox_id()))
}

pub fn identity_json(keys: &StoredProfileKeys) -> Result<String> {
    Ok(serde_json::to_string(&IdentityBundlePublic {
        sign: keys.identity_sign_public.clone(),
        dh: keys.identity_dh_public.clone(),
    })?)
}

pub fn parse_identity_json(value: &str) -> Result<IdentityBundlePublic> {
    serde_json::from_str(value).context("invalid identity_key payload")
}

pub fn generate_more_prekeys(count: usize) -> Vec<StoredOneTimePrekey> {
    let mut rng = OsRng;
    let mut prekeys = Vec::with_capacity(count);
    for _ in 0..count {
        let secret = StaticSecret::random_from_rng(&mut rng);
        let public = X25519PublicKey::from(&secret);
        prekeys.push(StoredOneTimePrekey {
            public_key: encode_bytes(public.as_bytes()),
            private_key: encode_bytes(secret.to_bytes().as_slice()),
            uploaded: true,
            consumed: false,
        });
    }
    prekeys
}

pub fn available_prekeys(keys: &StoredProfileKeys) -> usize {
    keys.one_time_prekeys
        .iter()
        .filter(|prekey| prekey.uploaded && !prekey.consumed)
        .count()
}

fn random_inbox_id() -> String {
    let mut bytes = [0u8; 18];
    OsRng.fill_bytes(&mut bytes);
    encode_bytes(&bytes)
}
