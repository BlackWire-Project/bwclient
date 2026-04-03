use uuid::Uuid;

use crate::relay::RelayBundle;
use crate::state::StoredProfileKeys;

use super::{
    bootstrap::{prepare_initial_message, receive_prekey_message},
    keys::{generate_profile_material, identity_json},
    ratchet::{prepare_session_message, receive_session_message, rotate_local_ratchet},
};

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
