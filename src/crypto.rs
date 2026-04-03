use anyhow::{Context, Result, anyhow, bail};
use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use chacha20poly1305::{
    ChaCha20Poly1305, KeyInit,
    aead::{Aead, Payload},
};
use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey};
use hkdf::Hkdf;
use rand::{RngCore, rngs::OsRng};
use serde::{Deserialize, Serialize};
use sha2::Sha256;
use uuid::Uuid;
use x25519_dalek::{PublicKey as X25519PublicKey, StaticSecret};

use crate::{
    relay::RelayBundle,
    state::{SessionState, StoredOneTimePrekey, StoredProfileKeys},
};

const PROTOCOL_VERSION: u8 = 1;
const INFO_X3DH: &[u8] = b"blackwire/v1/x3dh";
const INFO_INITIATOR_SEND: &[u8] = b"blackwire/v1/init/send";
const INFO_ROOT: &[u8] = b"blackwire/v1/root";
const INFO_CHAIN: &[u8] = b"blackwire/v1/chain";

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct IdentityBundlePublic {
    pub sign: String,
    pub dh: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct EncryptedPayload {
    pub text: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MessageHeader {
    pub version: u8,
    pub sender_username: String,
    pub sender_inbox_id: String,
    pub message_type: String,
    pub session_id: String,
    pub sender_identity_sign: String,
    pub sender_identity_dh: String,
    pub dh_pub: String,
    pub recipient_signed_prekey: Option<String>,
    pub used_one_time_prekey: Option<String>,
    pub pn: u32,
    pub n: u32,
    pub timestamp: String,
}

#[derive(Clone, Debug)]
pub struct RemoteBundleKeys {
    pub username: String,
    pub inbox_id: String,
    pub identity_sign_public: String,
    pub identity_dh_public: String,
    pub signed_prekey_public: String,
    pub one_time_prekey_public: Option<String>,
    pub prekey_id: Option<String>,
}

#[derive(Clone, Debug)]
pub struct PreparedMessage {
    pub relay_kind: String,
    pub relay_inbox_id: String,
    pub header_json: String,
    pub ciphertext: String,
    pub client_message_id: String,
    pub used_prekey_id: Option<String>,
    pub plaintext: String,
    pub session: SessionState,
}

#[derive(Clone, Debug)]
pub struct DecryptedIncoming {
    pub header: MessageHeader,
    pub plaintext: String,
    pub session: SessionState,
    pub sender_identity_json: String,
}

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

pub fn parse_remote_bundle(bundle: &RelayBundle) -> Result<RemoteBundleKeys> {
    let identity = parse_identity_json(&bundle.identity_key)?;
    verify_signed_prekey(
        &identity.sign,
        &bundle.signed_prekey,
        &bundle.signed_prekey_signature,
    )?;
    Ok(RemoteBundleKeys {
        username: bundle.username.clone(),
        inbox_id: bundle.inbox_id.clone(),
        identity_sign_public: identity.sign,
        identity_dh_public: identity.dh,
        signed_prekey_public: bundle.signed_prekey.clone(),
        one_time_prekey_public: bundle.one_time_prekey.clone(),
        prekey_id: bundle.prekey_id.clone(),
    })
}

pub fn verify_signed_prekey(
    identity_sign_public: &str,
    signed_prekey_public: &str,
    signature: &str,
) -> Result<()> {
    let verifying_key = verifying_key_from_b64(identity_sign_public)?;
    let signed_prekey_bytes = decode_32(signed_prekey_public)?;
    let signature = Signature::from_slice(&decode_bytes(signature)?)?;
    verifying_key
        .verify(&signed_prekey_bytes, &signature)
        .context("invalid signed_prekey signature")?;
    Ok(())
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

pub fn prepare_initial_message(
    local_username: &str,
    local_inbox_id: &str,
    local_keys: &StoredProfileKeys,
    remote_bundle: &RelayBundle,
    plaintext: &str,
) -> Result<PreparedMessage> {
    let remote = parse_remote_bundle(remote_bundle)?;
    let local_identity_dh = static_secret_from_b64(&local_keys.identity_dh_secret)?;
    let local_identity = signing_key_from_b64(&local_keys.identity_sign_secret)?;
    let remote_signed_prekey = x25519_public_from_b64(&remote.signed_prekey_public)?;
    let remote_identity_dh = x25519_public_from_b64(&remote.identity_dh_public)?;
    let mut rng = OsRng;
    let eph_secret = StaticSecret::random_from_rng(&mut rng);
    let eph_public = X25519PublicKey::from(&eph_secret);

    let mut shared = Vec::with_capacity(128);
    shared.extend_from_slice(
        local_identity_dh
            .diffie_hellman(&remote_signed_prekey)
            .as_bytes(),
    );
    shared.extend_from_slice(eph_secret.diffie_hellman(&remote_identity_dh).as_bytes());
    shared.extend_from_slice(eph_secret.diffie_hellman(&remote_signed_prekey).as_bytes());

    if let Some(one_time_prekey) = &remote.one_time_prekey_public {
        let one_time = x25519_public_from_b64(one_time_prekey)?;
        shared.extend_from_slice(eph_secret.diffie_hellman(&one_time).as_bytes());
    }

    let root_key = derive_bytes(&shared, INFO_X3DH)?;
    let initial_chain = derive_bytes(&root_key, INFO_INITIATOR_SEND)?;
    let (next_chain, message_key) = kdf_chain(&initial_chain)?;
    let session_id = Uuid::new_v4().to_string();
    let header = MessageHeader {
        version: PROTOCOL_VERSION,
        sender_username: local_username.to_string(),
        sender_inbox_id: local_inbox_id.to_string(),
        message_type: "prekey_message".to_string(),
        session_id: session_id.clone(),
        sender_identity_sign: encode_bytes(local_identity.verifying_key().as_bytes()),
        sender_identity_dh: local_keys.identity_dh_public.clone(),
        dh_pub: encode_bytes(eph_public.as_bytes()),
        recipient_signed_prekey: Some(remote.signed_prekey_public.clone()),
        used_one_time_prekey: remote.one_time_prekey_public.clone(),
        pn: 0,
        n: 0,
        timestamp: now_iso(),
    };
    let header_json = serde_json::to_string(&header)?;
    let ciphertext = encrypt_text(
        &message_key,
        &header_json,
        &EncryptedPayload {
            text: plaintext.to_string(),
        },
    )?;

    Ok(PreparedMessage {
        relay_kind: "prekey_message".to_string(),
        relay_inbox_id: remote.inbox_id.clone(),
        header_json,
        ciphertext,
        client_message_id: Uuid::new_v4().to_string(),
        used_prekey_id: remote.prekey_id.clone(),
        plaintext: plaintext.to_string(),
        session: SessionState {
            version: PROTOCOL_VERSION,
            session_id,
            peer_username: Some(remote.username),
            peer_inbox_id: remote.inbox_id,
            peer_identity_sign_public: remote.identity_sign_public,
            peer_identity_dh_public: remote.identity_dh_public,
            root_key: encode_bytes(&root_key),
            sending_chain_key: Some(encode_bytes(&next_chain)),
            receiving_chain_key: None,
            local_ratchet_private: encode_bytes(eph_secret.to_bytes().as_slice()),
            local_ratchet_public: encode_bytes(eph_public.as_bytes()),
            remote_ratchet_public: None,
            send_count: 1,
            receive_count: 0,
            previous_send_count: 0,
            pending_send_ratchet: false,
        },
    })
}

pub fn prepare_session_message(
    session: &SessionState,
    local_username: &str,
    local_inbox_id: &str,
    plaintext: &str,
) -> Result<PreparedMessage> {
    let mut next_session = session.clone();
    let remote_ratchet = next_session
        .remote_ratchet_public
        .as_ref()
        .map(|value| x25519_public_from_b64(value))
        .transpose()?;

    if next_session.pending_send_ratchet || next_session.sending_chain_key.is_none() {
        let remote_ratchet =
            remote_ratchet.ok_or_else(|| anyhow!("session is waiting for a remote ratchet key"))?;
        let local_ratchet = static_secret_from_b64(&next_session.local_ratchet_private)?;
        let (next_root, sending_chain) = kdf_root(
            &decode_bytes(&next_session.root_key)?,
            local_ratchet.diffie_hellman(&remote_ratchet).as_bytes(),
        )?;
        next_session.root_key = encode_bytes(&next_root);
        next_session.sending_chain_key = Some(encode_bytes(&sending_chain));
        next_session.pending_send_ratchet = false;
        next_session.previous_send_count = next_session.send_count;
        next_session.send_count = 0;
    }

    let sending_chain = next_session
        .sending_chain_key
        .clone()
        .ok_or_else(|| anyhow!("missing sending chain"))?;
    let (next_chain, message_key) = kdf_chain(&decode_bytes(&sending_chain)?)?;
    next_session.sending_chain_key = Some(encode_bytes(&next_chain));
    let message_index = next_session.send_count;
    next_session.send_count += 1;

    let header = MessageHeader {
        version: PROTOCOL_VERSION,
        sender_username: local_username.to_string(),
        sender_inbox_id: local_inbox_id.to_string(),
        message_type: "ratchet_message".to_string(),
        session_id: next_session.session_id.clone(),
        sender_identity_sign: next_session.peer_identity_sign_public.clone(),
        sender_identity_dh: String::new(),
        dh_pub: next_session.local_ratchet_public.clone(),
        recipient_signed_prekey: None,
        used_one_time_prekey: None,
        pn: next_session.previous_send_count,
        n: message_index,
        timestamp: now_iso(),
    };
    let mut header = header;
    header.sender_identity_sign = String::new();
    header.sender_identity_dh = String::new();
    let header_json = serde_json::to_string(&header)?;
    let ciphertext = encrypt_text(
        &message_key,
        &header_json,
        &EncryptedPayload {
            text: plaintext.to_string(),
        },
    )?;

    Ok(PreparedMessage {
        relay_kind: "ratchet_message".to_string(),
        relay_inbox_id: next_session.peer_inbox_id.clone(),
        header_json,
        ciphertext,
        client_message_id: Uuid::new_v4().to_string(),
        used_prekey_id: None,
        plaintext: plaintext.to_string(),
        session: next_session,
    })
}

pub fn receive_prekey_message(
    local_keys: &mut StoredProfileKeys,
    local_username: &str,
    local_inbox_id: &str,
    header_json: &str,
    ciphertext: &str,
) -> Result<DecryptedIncoming> {
    let header: MessageHeader = serde_json::from_str(header_json)?;
    if header.message_type != "prekey_message" {
        bail!("expected prekey_message header");
    }

    let sender_identity_sign = header.sender_identity_sign.clone();
    let sender_identity_dh = header.sender_identity_dh.clone();
    let sender_ephemeral = x25519_public_from_b64(&header.dh_pub)?;
    let local_signed_prekey = static_secret_from_b64(&local_keys.signed_prekey_secret)?;
    let local_identity_dh = static_secret_from_b64(&local_keys.identity_dh_secret)?;
    let sender_identity_dh_public = x25519_public_from_b64(&sender_identity_dh)?;

    let mut shared = Vec::with_capacity(128);
    shared.extend_from_slice(
        local_signed_prekey
            .diffie_hellman(&sender_identity_dh_public)
            .as_bytes(),
    );
    shared.extend_from_slice(
        local_identity_dh
            .diffie_hellman(&sender_ephemeral)
            .as_bytes(),
    );
    shared.extend_from_slice(
        local_signed_prekey
            .diffie_hellman(&sender_ephemeral)
            .as_bytes(),
    );

    if let Some(one_time_prekey_public) = &header.used_one_time_prekey {
        let one_time = local_keys
            .one_time_prekeys
            .iter_mut()
            .find(|candidate| candidate.public_key == *one_time_prekey_public)
            .ok_or_else(|| anyhow!("missing one-time prekey for incoming message"))?;
        let one_time_secret = static_secret_from_b64(&one_time.private_key)?;
        shared.extend_from_slice(one_time_secret.diffie_hellman(&sender_ephemeral).as_bytes());
        one_time.consumed = true;
    }

    let root_key = derive_bytes(&shared, INFO_X3DH)?;
    let initial_chain = derive_bytes(&root_key, INFO_INITIATOR_SEND)?;
    let (next_chain, message_key) = kdf_chain(&initial_chain)?;
    let plaintext: EncryptedPayload = decrypt_text(&message_key, header_json, ciphertext)?;

    let mut rng = OsRng;
    let local_ratchet = StaticSecret::random_from_rng(&mut rng);
    let local_ratchet_public = X25519PublicKey::from(&local_ratchet);

    let sender_identity_json = serde_json::to_string(&IdentityBundlePublic {
        sign: sender_identity_sign.clone(),
        dh: sender_identity_dh.clone(),
    })?;

    Ok(DecryptedIncoming {
        header: header.clone(),
        plaintext: plaintext.text,
        sender_identity_json,
        session: SessionState {
            version: PROTOCOL_VERSION,
            session_id: Uuid::new_v4().to_string(),
            peer_username: None,
            peer_inbox_id: local_inbox_id.to_string(),
            peer_identity_sign_public: sender_identity_sign,
            peer_identity_dh_public: sender_identity_dh,
            root_key: encode_bytes(&root_key),
            sending_chain_key: None,
            receiving_chain_key: Some(encode_bytes(&next_chain)),
            local_ratchet_private: encode_bytes(local_ratchet.to_bytes().as_slice()),
            local_ratchet_public: encode_bytes(local_ratchet_public.as_bytes()),
            remote_ratchet_public: Some(header.dh_pub.clone()),
            send_count: 0,
            receive_count: 1,
            previous_send_count: 0,
            pending_send_ratchet: true,
        },
    })
}

pub fn receive_session_message(
    session: &SessionState,
    header_json: &str,
    ciphertext: &str,
) -> Result<DecryptedIncoming> {
    let header: MessageHeader = serde_json::from_str(header_json)?;
    if header.message_type != "ratchet_message" {
        bail!("expected ratchet_message header");
    }

    let mut next_session = session.clone();
    if next_session.remote_ratchet_public.as_deref() != Some(header.dh_pub.as_str()) {
        let local_ratchet = static_secret_from_b64(&next_session.local_ratchet_private)?;
        let remote_ratchet = x25519_public_from_b64(&header.dh_pub)?;
        let (next_root, receiving_chain) = kdf_root(
            &decode_bytes(&next_session.root_key)?,
            local_ratchet.diffie_hellman(&remote_ratchet).as_bytes(),
        )?;
        next_session.root_key = encode_bytes(&next_root);
        next_session.remote_ratchet_public = Some(header.dh_pub.clone());
        next_session.receiving_chain_key = Some(encode_bytes(&receiving_chain));
        next_session.receive_count = 0;
        next_session.pending_send_ratchet = true;
    }

    let receiving_chain = next_session
        .receiving_chain_key
        .clone()
        .ok_or_else(|| anyhow!("missing receiving chain"))?;
    let (next_chain, message_key) = kdf_chain(&decode_bytes(&receiving_chain)?)?;
    let plaintext: EncryptedPayload = decrypt_text(&message_key, header_json, ciphertext)?;
    next_session.receiving_chain_key = Some(encode_bytes(&next_chain));
    next_session.receive_count += 1;

    Ok(DecryptedIncoming {
        sender_identity_json: String::new(),
        header,
        plaintext: plaintext.text,
        session: next_session,
    })
}

pub fn rotate_local_ratchet(session: &mut SessionState) -> Result<()> {
    let mut rng = OsRng;
    let local_ratchet = StaticSecret::random_from_rng(&mut rng);
    let local_ratchet_public = X25519PublicKey::from(&local_ratchet);
    session.local_ratchet_private = encode_bytes(local_ratchet.to_bytes().as_slice());
    session.local_ratchet_public = encode_bytes(local_ratchet_public.as_bytes());
    Ok(())
}

fn encrypt_text(key: &[u8], aad: &str, payload: &EncryptedPayload) -> Result<String> {
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

fn decrypt_text<T: for<'de> Deserialize<'de>>(
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

fn derive_bytes(input: &[u8], info: &[u8]) -> Result<Vec<u8>> {
    let hk = Hkdf::<Sha256>::new(None, input);
    let mut out = vec![0u8; 32];
    hk.expand(info, &mut out)
        .map_err(|_| anyhow!("hkdf derive failed"))?;
    Ok(out)
}

fn kdf_root(root_key: &[u8], dh_out: &[u8]) -> Result<(Vec<u8>, Vec<u8>)> {
    let hk = Hkdf::<Sha256>::new(Some(root_key), dh_out);
    let mut out = [0u8; 64];
    hk.expand(INFO_ROOT, &mut out)
        .map_err(|_| anyhow!("hkdf root derive failed"))?;
    Ok((out[..32].to_vec(), out[32..].to_vec()))
}

fn kdf_chain(chain_key: &[u8]) -> Result<(Vec<u8>, Vec<u8>)> {
    let hk = Hkdf::<Sha256>::new(Some(chain_key), b"");
    let mut out = [0u8; 64];
    hk.expand(INFO_CHAIN, &mut out)
        .map_err(|_| anyhow!("hkdf chain derive failed"))?;
    Ok((out[..32].to_vec(), out[32..].to_vec()))
}

fn signing_key_from_b64(value: &str) -> Result<SigningKey> {
    Ok(SigningKey::from_bytes(&decode_32(value)?))
}

fn verifying_key_from_b64(value: &str) -> Result<VerifyingKey> {
    Ok(VerifyingKey::from_bytes(&decode_32(value)?)?)
}

fn static_secret_from_b64(value: &str) -> Result<StaticSecret> {
    Ok(StaticSecret::from(decode_32(value)?))
}

fn x25519_public_from_b64(value: &str) -> Result<X25519PublicKey> {
    Ok(X25519PublicKey::from(decode_32(value)?))
}

fn decode_32(value: &str) -> Result<[u8; 32]> {
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

fn random_inbox_id() -> String {
    let mut bytes = [0u8; 48];
    OsRng.fill_bytes(&mut bytes);
    encode_bytes(&bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::relay::RelayBundle;

    fn bundle(username: &str, inbox_id: &str, keys: &StoredProfileKeys) -> RelayBundle {
        RelayBundle {
            username: username.to_string(),
            identity_key: identity_json(keys).unwrap(),
            signed_prekey: keys.signed_prekey_public.clone(),
            signed_prekey_signature: keys.signed_prekey_signature.clone(),
            inbox_id: inbox_id.to_string(),
            prekey_id: Some(Uuid::new_v4().to_string()),
            one_time_prekey: Some(keys.one_time_prekeys[0].public_key.clone()),
        }
    }

    #[test]
    fn random_inbox_id_has_higher_entropy_payload_length() {
        let inbox_id = random_inbox_id();
        let decoded = decode_bytes(&inbox_id).unwrap();

        assert_eq!(decoded.len(), 48);
    }

    fn tamper_header<F>(header_json: &str, mutate: F) -> String
    where
        F: FnOnce(&mut MessageHeader),
    {
        let mut header: MessageHeader = serde_json::from_str(header_json).unwrap();
        mutate(&mut header);
        serde_json::to_string(&header).unwrap()
    }

    #[test]
    fn roundtrip_prekey_and_ratchet() {
        let (alice_keys, alice_inbox) = generate_profile_material(4).unwrap();
        let (mut bob_keys, bob_inbox) = generate_profile_material(4).unwrap();

        let bob_bundle = bundle("bob", &bob_inbox, &bob_keys);
        let alice_out =
            prepare_initial_message("alice", &alice_inbox, &alice_keys, &bob_bundle, "hello")
                .unwrap();

        let bob_in = receive_prekey_message(
            &mut bob_keys,
            "bob",
            &bob_inbox,
            &alice_out.header_json,
            &alice_out.ciphertext,
        )
        .unwrap();
        assert_eq!(bob_in.plaintext, "hello");

        let mut bob_session = bob_in.session;
        bob_session.session_id = bob_in.header.session_id.clone();
        bob_session.peer_username = Some("alice".to_string());
        bob_session.peer_inbox_id = alice_inbox.clone();

        rotate_local_ratchet(&mut bob_session).unwrap();
        let bob_reply = prepare_session_message(&bob_session, "bob", &bob_inbox, "reply").unwrap();

        let alice_in = receive_session_message(
            &alice_out.session,
            &bob_reply.header_json,
            &bob_reply.ciphertext,
        )
        .unwrap();
        assert_eq!(alice_in.plaintext, "reply");
    }

    #[test]
    fn verify_signed_prekey_rejects_tampered_signature() {
        let (keys, _) = generate_profile_material(1).unwrap();
        let mut signature = decode_bytes(&keys.signed_prekey_signature).unwrap();
        signature[0] ^= 0x01;

        let error = verify_signed_prekey(
            &keys.identity_sign_public,
            &keys.signed_prekey_public,
            &encode_bytes(&signature),
        )
        .unwrap_err();

        assert!(error.to_string().contains("invalid signed_prekey signature"));
    }

    #[test]
    fn verify_signed_prekey_rejects_tampered_signed_prekey() {
        let (keys, _) = generate_profile_material(1).unwrap();
        let mut public = decode_32(&keys.signed_prekey_public).unwrap();
        public[0] ^= 0x01;

        let error = verify_signed_prekey(
            &keys.identity_sign_public,
            &encode_bytes(&public),
            &keys.signed_prekey_signature,
        )
        .unwrap_err();

        assert!(error.to_string().contains("invalid signed_prekey signature"));
    }

    #[test]
    fn verify_signed_prekey_rejects_wrong_identity_signer() {
        let (keys, _) = generate_profile_material(1).unwrap();
        let (other_keys, _) = generate_profile_material(1).unwrap();

        let error = verify_signed_prekey(
            &other_keys.identity_sign_public,
            &keys.signed_prekey_public,
            &keys.signed_prekey_signature,
        )
        .unwrap_err();

        assert!(error.to_string().contains("invalid signed_prekey signature"));
    }

    #[test]
    fn receive_prekey_message_rejects_wrong_message_type() {
        let (alice_keys, alice_inbox) = generate_profile_material(4).unwrap();
        let (mut bob_keys, bob_inbox) = generate_profile_material(4).unwrap();
        let bob_bundle = bundle("bob", &bob_inbox, &bob_keys);
        let alice_out =
            prepare_initial_message("alice", &alice_inbox, &alice_keys, &bob_bundle, "hello")
                .unwrap();
        let tampered = tamper_header(&alice_out.header_json, |header| {
            header.message_type = "ratchet_message".to_string();
        });

        let error = receive_prekey_message(
            &mut bob_keys,
            "bob",
            &bob_inbox,
            &tampered,
            &alice_out.ciphertext,
        )
        .unwrap_err();

        assert!(error.to_string().contains("expected prekey_message header"));
    }

    #[test]
    fn receive_prekey_message_rejects_missing_one_time_prekey() {
        let (alice_keys, alice_inbox) = generate_profile_material(4).unwrap();
        let (mut bob_keys, bob_inbox) = generate_profile_material(4).unwrap();
        let bob_bundle = bundle("bob", &bob_inbox, &bob_keys);
        let alice_out =
            prepare_initial_message("alice", &alice_inbox, &alice_keys, &bob_bundle, "hello")
                .unwrap();
        let tampered = tamper_header(&alice_out.header_json, |header| {
            header.used_one_time_prekey = Some(encode_bytes(b"missing-prekey-material"));
        });

        let error = receive_prekey_message(
            &mut bob_keys,
            "bob",
            &bob_inbox,
            &tampered,
            &alice_out.ciphertext,
        )
        .unwrap_err();

        assert!(error
            .to_string()
            .contains("missing one-time prekey for incoming message"));
    }

    #[test]
    fn receive_prekey_message_rejects_tampered_ciphertext() {
        let (alice_keys, alice_inbox) = generate_profile_material(4).unwrap();
        let (mut bob_keys, bob_inbox) = generate_profile_material(4).unwrap();
        let bob_bundle = bundle("bob", &bob_inbox, &bob_keys);
        let alice_out =
            prepare_initial_message("alice", &alice_inbox, &alice_keys, &bob_bundle, "hello")
                .unwrap();
        let mut ciphertext = decode_bytes(&alice_out.ciphertext).unwrap();
        let last = ciphertext.len() - 1;
        ciphertext[last] ^= 0x01;

        let error = receive_prekey_message(
            &mut bob_keys,
            "bob",
            &bob_inbox,
            &alice_out.header_json,
            &encode_bytes(&ciphertext),
        )
        .unwrap_err();

        assert!(!error.to_string().is_empty());
    }

    #[test]
    fn receive_prekey_message_accepts_arbitrary_sender_username_with_same_keys() {
        let (alice_keys, alice_inbox) = generate_profile_material(4).unwrap();
        let (mut bob_keys, bob_inbox) = generate_profile_material(4).unwrap();
        let bob_bundle = bundle("bob", &bob_inbox, &bob_keys);
        let spoofed =
            prepare_initial_message("mallory", &alice_inbox, &alice_keys, &bob_bundle, "hello")
                .unwrap();
        let received = receive_prekey_message(
            &mut bob_keys,
            "bob",
            &bob_inbox,
            &spoofed.header_json,
            &spoofed.ciphertext,
        )
        .unwrap();

        assert_eq!(received.plaintext, "hello");
        assert_eq!(received.header.sender_username, "mallory");
    }

    #[test]
    fn receive_session_message_rejects_wrong_message_type() {
        let (alice_keys, alice_inbox) = generate_profile_material(4).unwrap();
        let (mut bob_keys, bob_inbox) = generate_profile_material(4).unwrap();
        let bob_bundle = bundle("bob", &bob_inbox, &bob_keys);
        let alice_out =
            prepare_initial_message("alice", &alice_inbox, &alice_keys, &bob_bundle, "hello")
                .unwrap();
        let bob_in = receive_prekey_message(
            &mut bob_keys,
            "bob",
            &bob_inbox,
            &alice_out.header_json,
            &alice_out.ciphertext,
        )
        .unwrap();
        let mut bob_session = bob_in.session;
        bob_session.session_id = bob_in.header.session_id.clone();
        bob_session.peer_username = Some("alice".to_string());
        bob_session.peer_inbox_id = alice_inbox.clone();
        rotate_local_ratchet(&mut bob_session).unwrap();
        let bob_reply = prepare_session_message(&bob_session, "bob", &bob_inbox, "reply").unwrap();
        let tampered = tamper_header(&bob_reply.header_json, |header| {
            header.message_type = "prekey_message".to_string();
        });

        let error =
            receive_session_message(&alice_out.session, &tampered, &bob_reply.ciphertext).unwrap_err();
        assert!(error.to_string().contains("expected ratchet_message header"));
    }

    #[test]
    fn receive_session_message_rejects_tampered_ciphertext() {
        let (alice_keys, alice_inbox) = generate_profile_material(4).unwrap();
        let (mut bob_keys, bob_inbox) = generate_profile_material(4).unwrap();
        let bob_bundle = bundle("bob", &bob_inbox, &bob_keys);
        let alice_out =
            prepare_initial_message("alice", &alice_inbox, &alice_keys, &bob_bundle, "hello")
                .unwrap();
        let bob_in = receive_prekey_message(
            &mut bob_keys,
            "bob",
            &bob_inbox,
            &alice_out.header_json,
            &alice_out.ciphertext,
        )
        .unwrap();
        let mut bob_session = bob_in.session;
        bob_session.session_id = bob_in.header.session_id.clone();
        bob_session.peer_username = Some("alice".to_string());
        bob_session.peer_inbox_id = alice_inbox.clone();
        rotate_local_ratchet(&mut bob_session).unwrap();
        let bob_reply = prepare_session_message(&bob_session, "bob", &bob_inbox, "reply").unwrap();
        let mut ciphertext = decode_bytes(&bob_reply.ciphertext).unwrap();
        ciphertext[0] ^= 0x01;

        let error = receive_session_message(
            &alice_out.session,
            &bob_reply.header_json,
            &encode_bytes(&ciphertext),
        )
        .unwrap_err();
        assert!(!error.to_string().is_empty());
    }

    #[test]
    fn receive_session_message_rejects_wrong_session_state() {
        let (alice_keys, alice_inbox) = generate_profile_material(4).unwrap();
        let (mut bob_keys, bob_inbox) = generate_profile_material(4).unwrap();
        let (eve_keys, eve_inbox) = generate_profile_material(4).unwrap();
        let bob_bundle = bundle("bob", &bob_inbox, &bob_keys);
        let eve_bundle = bundle("eve", &eve_inbox, &eve_keys);
        let alice_out =
            prepare_initial_message("alice", &alice_inbox, &alice_keys, &bob_bundle, "hello")
                .unwrap();
        let bob_in = receive_prekey_message(
            &mut bob_keys,
            "bob",
            &bob_inbox,
            &alice_out.header_json,
            &alice_out.ciphertext,
        )
        .unwrap();
        let mut bob_session = bob_in.session;
        bob_session.session_id = bob_in.header.session_id.clone();
        bob_session.peer_username = Some("alice".to_string());
        bob_session.peer_inbox_id = alice_inbox.clone();
        rotate_local_ratchet(&mut bob_session).unwrap();
        let bob_reply = prepare_session_message(&bob_session, "bob", &bob_inbox, "reply").unwrap();
        let wrong_session =
            prepare_initial_message("alice", &alice_inbox, &alice_keys, &eve_bundle, "other")
                .unwrap()
                .session;

        assert!(receive_session_message(&wrong_session, &bob_reply.header_json, &bob_reply.ciphertext).is_err());
    }
}
