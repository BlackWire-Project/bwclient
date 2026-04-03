use anyhow::{Result, anyhow, bail};
use rand::rngs::OsRng;
use uuid::Uuid;
use x25519_dalek::{PublicKey as X25519PublicKey, StaticSecret};

use crate::state::SessionState;

use super::{
    constants::PROTOCOL_VERSION,
    keys::now_iso,
    primitives::{
        decode_bytes, decrypt_text, encode_bytes, encrypt_text, kdf_chain, kdf_root,
        static_secret_from_b64, x25519_public_from_b64,
    },
    types::{DecryptedIncoming, EncryptedPayload, MessageHeader, PreparedMessage},
};

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

    let mut header = MessageHeader {
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
