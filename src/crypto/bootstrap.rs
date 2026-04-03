use anyhow::{Context, Result, anyhow, bail};
use ed25519_dalek::{Signature, Verifier};
use rand::rngs::OsRng;
use uuid::Uuid;
use x25519_dalek::{PublicKey as X25519PublicKey, StaticSecret};

use crate::{
    relay::RelayBundle,
    state::{SessionState, StoredProfileKeys},
};

use super::{
    constants::{INFO_INITIATOR_SEND, INFO_X3DH, PROTOCOL_VERSION},
    keys::{now_iso, parse_identity_json},
    primitives::{
        decode_bytes, decode_32, derive_bytes, encrypt_text, kdf_chain, signing_key_from_b64,
        static_secret_from_b64, verifying_key_from_b64, x25519_public_from_b64,
    },
    types::{
        DecryptedIncoming, EncryptedPayload, IdentityBundlePublic, MessageHeader, PreparedMessage,
        RemoteBundleKeys,
    },
};

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
        sender_identity_sign: super::primitives::encode_bytes(local_identity.verifying_key().as_bytes()),
        sender_identity_dh: local_keys.identity_dh_public.clone(),
        dh_pub: super::primitives::encode_bytes(eph_public.as_bytes()),
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
            root_key: super::primitives::encode_bytes(&root_key),
            sending_chain_key: Some(super::primitives::encode_bytes(&next_chain)),
            receiving_chain_key: None,
            local_ratchet_private: super::primitives::encode_bytes(eph_secret.to_bytes().as_slice()),
            local_ratchet_public: super::primitives::encode_bytes(eph_public.as_bytes()),
            remote_ratchet_public: None,
            send_count: 1,
            receive_count: 0,
            previous_send_count: 0,
            pending_send_ratchet: false,
        },
    })
}

pub fn receive_prekey_message(
    local_keys: &mut StoredProfileKeys,
    _local_username: &str,
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
    let plaintext: EncryptedPayload =
        super::primitives::decrypt_text(&message_key, header_json, ciphertext)?;

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
            root_key: super::primitives::encode_bytes(&root_key),
            sending_chain_key: None,
            receiving_chain_key: Some(super::primitives::encode_bytes(&next_chain)),
            local_ratchet_private: super::primitives::encode_bytes(local_ratchet.to_bytes().as_slice()),
            local_ratchet_public: super::primitives::encode_bytes(local_ratchet_public.as_bytes()),
            remote_ratchet_public: Some(header.dh_pub.clone()),
            send_count: 0,
            receive_count: 1,
            previous_send_count: 0,
            pending_send_ratchet: true,
        },
    })
}
