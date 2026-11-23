//! Integration tests for the protocol state machine shared by every frontend.
//!
//! These exercise `ClientCore` exactly the way the CLI and web app do: by
//! exchanging `PeerFrame`s and reacting to `Action`s, with no transport.

use encchat_core::{Action, ClientCore, DeviceId, PeerFrame, Psk};

fn first_send(actions: Vec<Action>, to: DeviceId) -> PeerFrame {
    actions
        .into_iter()
        .find_map(|action| match action {
            Action::Send { to: dest, frame } if dest == to => Some(frame),
            _ => None,
        })
        .expect("expected an Action::Send for the peer")
}

/// Run the three-message handshake between two cores, asserting both sides
/// install a session and the initiator observes completion.
fn connect(a: &mut ClientCore, b: &mut ClientCore) {
    let (a_id, b_id) = (a.device_id(), b.device_id());

    let m0 = a.begin_handshake(b_id).unwrap();
    let m1 = first_send(b.handle_frame(a_id, m0).unwrap(), a_id);

    let actions = a.handle_frame(b_id, m1).unwrap();
    assert!(
        actions
            .iter()
            .any(|action| matches!(action, Action::HandshakeComplete { with } if *with == b_id)),
        "initiator should finish after the reply"
    );
    let m2 = first_send(actions, b_id);

    let actions = b.handle_frame(a_id, m2).unwrap();
    assert!(
        actions
            .iter()
            .any(|action| matches!(action, Action::HandshakeComplete { with } if *with == a_id)),
        "responder should finish after the final message"
    );

    assert!(a.has_session(b_id));
    assert!(b.has_session(a_id));
}

#[test]
fn nickname_is_announced_inside_the_ratchet() {
    let psk = Psk::generate().unwrap();
    let mut alice = ClientCore::new(psk.clone()).unwrap();
    let mut bob = ClientCore::new(psk).unwrap();
    let (a, b) = (alice.device_id(), bob.device_id());
    connect(&mut alice, &mut bob);

    assert!(alice.set_nick("alice").is_some());
    let frame = alice.announce_nick(b).expect("has a session");

    // The nickname must be encrypted, never a plaintext peer frame: the relay
    // sees room/device ids and ciphertext only.
    assert!(matches!(frame, PeerFrame::Message { .. }));
    assert!(!format!("{frame:?}").contains("alice"));

    bob.handle_frame(a, frame).unwrap();
    assert_eq!(bob.peer_nick(a), Some("alice"));
    assert_eq!(bob.peer_nicks(), vec![(a, "alice".to_string())]);
    // A nickname announcement is not a chat message.
    assert!(bob.history().is_empty());

    // An empty or whitespace-only nick is rejected.
    assert!(alice.set_nick("   ").is_none());
    // Control characters are stripped and the length is capped at 32.
    let messy = format!("{}\n\tbad", "x".repeat(100));
    assert!(alice.set_nick(&messy).is_some());
    let frame = alice.announce_nick(b).unwrap();
    bob.handle_frame(a, frame).unwrap();
    let received = bob.peer_nick(a).unwrap();
    assert_eq!(received.len(), 32);
    assert!(!received.contains('\n') && !received.contains('\t'));
}

#[test]
fn announcing_a_nick_requires_a_session_and_a_nick() {
    let psk = Psk::generate().unwrap();
    let mut alice = ClientCore::new(psk.clone()).unwrap();
    let bob = ClientCore::new(psk).unwrap();

    // No nickname set yet.
    assert!(alice.announce_nick(bob.device_id()).is_none());
    assert!(alice.set_nick("alice").is_some());
    // No session with that peer yet.
    assert!(alice.announce_nick(bob.device_id()).is_none());
}

#[test]
fn two_clients_handshake_and_exchange_messages() {
    let psk = Psk::generate().unwrap();
    let mut alice = ClientCore::new(psk.clone()).unwrap();
    let mut bob = ClientCore::new(psk).unwrap();
    let (a, b) = (alice.device_id(), bob.device_id());

    connect(&mut alice, &mut bob);

    let frame = alice.send_text(b, "hello bob", 1_700_000_000_000).unwrap();
    let actions = bob.handle_frame(a, frame).unwrap();
    assert!(actions.iter().any(|action| matches!(
        action,
        Action::Received { body, .. } if body == "hello bob"
    )));

    // Reply in the other direction, forcing a DH ratchet.
    let frame = bob.send_text(a, "hello alice", 1_700_000_001_000).unwrap();
    let actions = alice.handle_frame(b, frame).unwrap();
    assert!(actions.iter().any(|action| matches!(
        action,
        Action::Received { body, .. } if body == "hello alice"
    )));

    assert_eq!(alice.history().len(), 2);
    assert_eq!(bob.history().len(), 2);
    assert!(alice.history().iter().any(|e| e.outgoing && e.body == "hello bob"));
    assert!(bob.history().iter().any(|e| !e.outgoing && e.body == "hello bob"));
}

#[test]
fn multi_device_fanout_reaches_each_peer_device() {
    let psk = Psk::generate().unwrap();
    let mut alice = ClientCore::new(psk.clone()).unwrap();
    let mut bob_laptop = ClientCore::new(psk.clone()).unwrap();
    let mut bob_phone = ClientCore::new(psk.clone()).unwrap();

    connect(&mut alice, &mut bob_laptop);
    connect(&mut alice, &mut bob_phone);

    let peers = [bob_laptop.device_id(), bob_phone.device_id()];
    let out = alice
        .send_text_to_all(&peers, "to both devices", 42)
        .unwrap();
    assert_eq!(out.len(), 2);

    for (to, frame) in out {
        let bob = if to == bob_laptop.device_id() {
            &mut bob_laptop
        } else {
            &mut bob_phone
        };
        let actions = bob.handle_frame(alice.device_id(), frame).unwrap();
        assert!(actions.iter().any(|action| matches!(
            action,
            Action::Received { body, .. } if body == "to both devices"
        )));
    }
}

#[test]
fn each_direction_has_independent_chain_state() {
    let psk = Psk::generate().unwrap();
    let mut alice = ClientCore::new(psk.clone()).unwrap();
    let mut bob = ClientCore::new(psk).unwrap();
    let (a, b) = (alice.device_id(), bob.device_id());
    connect(&mut alice, &mut bob);

    // Interleave many messages in both directions; the ratchet must stay in
    // sync and derive a distinct key per message.
    for i in 0..25u64 {
        let f = alice.send_text(b, &format!("a{i}"), i).unwrap();
        let actions = bob.handle_frame(a, f).unwrap();
        assert!(actions.iter().any(|x| matches!(x, Action::Received { .. })));

        let f = bob.send_text(a, &format!("b{i}"), i).unwrap();
        let actions = alice.handle_frame(b, f).unwrap();
        assert!(actions.iter().any(|x| matches!(x, Action::Received { .. })));
    }

    assert_eq!(alice.history().len(), 50);
    assert_eq!(bob.history().len(), 50);
}
