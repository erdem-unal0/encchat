//! Routing tests that drive `Relay` directly, without a network.

use std::time::Duration;

use encchat_core::{decode, DeviceId, Mac, RoomId, ServerFrame};
use encchat_server::{MailboxConfig, Relay};
use tokio::sync::mpsc::unbounded_channel;

fn config() -> MailboxConfig {
    MailboxConfig {
        max_entries: 8,
        ttl: Duration::from_secs(60),
    }
}

fn id(n: u8) -> DeviceId {
    [n; 16]
}

fn mac(n: u8) -> Mac {
    [n; 32]
}

/// Collect the frames a receiver was handed, decoded.
fn drain(rx: &mut tokio::sync::mpsc::UnboundedReceiver<Vec<u8>>) -> Vec<ServerFrame> {
    let mut out = Vec::new();
    while let Ok(bytes) = rx.try_recv() {
        out.push(decode(&bytes).expect("frame decodes"));
    }
    out
}

/// Decode a batch of already-encoded frames, as returned by `Relay::join`.
fn decode_all(frames: Vec<Vec<u8>>) -> Vec<ServerFrame> {
    frames
        .iter()
        .map(|bytes| decode(bytes).expect("frame decodes"))
        .collect()
}

#[test]
fn join_returns_device_list_including_self() {
    let room: RoomId = [7; 16];
    let mut relay = Relay::new(config());
    let (tx_a, _rx_a) = unbounded_channel();
    let (tx_b, _rx_b) = unbounded_channel();

    relay.join(room, id(1), mac(1), tx_a);
    let frames = decode_all(relay.join(room, id(2), mac(2), tx_b).to_self);

    match &frames[0] {
        ServerFrame::Joined { room: r, devices } => {
            assert_eq!(*r, room);
            assert_eq!(devices.len(), 2);
            assert!(devices.iter().any(|d| d.id == id(1)));
            assert!(devices.iter().any(|d| d.id == id(2)));
        }
        other => panic!("expected Joined, got {other:?}"),
    }
}

#[test]
fn delivers_directly_when_peer_is_online() {
    let room: RoomId = [7; 16];
    let mut relay = Relay::new(config());
    let (tx_a, _rx_a) = unbounded_channel();
    let (tx_b, mut rx_b) = unbounded_channel();

    relay.join(room, id(1), mac(1), tx_a);
    relay.join(room, id(2), mac(2), tx_b);
    let _ = drain(&mut rx_b); // discard B's Joined frame

    let delivered = relay.deliver(room, id(1), id(2), vec![9, 9, 9]);
    assert!(delivered, "online peer should receive immediately");

    match &drain(&mut rx_b)[0] {
        ServerFrame::Deliver { from, payload } => {
            assert_eq!(*from, id(1));
            assert_eq!(payload, &vec![9, 9, 9]);
        }
        other => panic!("expected Deliver, got {other:?}"),
    }
}

#[test]
fn queues_for_offline_peer_then_flushes_on_rejoin() {
    let room: RoomId = [7; 16];
    let mut relay = Relay::new(config());
    let (tx_a, _rx_a) = unbounded_channel();
    let (tx_b, _rx_b) = unbounded_channel();

    relay.join(room, id(1), mac(1), tx_a);
    relay.join(room, id(2), mac(2), tx_b);
    relay.disconnect(room, id(2));

    // B is offline: this must be queued, not delivered.
    let delivered = relay.deliver(room, id(1), id(2), vec![1, 2, 3]);
    assert!(!delivered, "offline peer should be queued");

    // B comes back: the queue is flushed right after the Joined frame.
    let (tx_b2, rx_b2) = unbounded_channel();
    let frames = decode_all(relay.join(room, id(2), mac(2), tx_b2).to_self);
    assert!(matches!(frames[0], ServerFrame::Joined { .. }));
    assert_eq!(frames.len(), 2, "Joined + one queued message");
    match &frames[1] {
        ServerFrame::Deliver { from, payload } => {
            assert_eq!(*from, id(1));
            assert_eq!(payload, &vec![1, 2, 3]);
        }
        other => panic!("expected queued Deliver, got {other:?}"),
    }
    let _ = rx_b2;
}

#[test]
fn deliver_to_unknown_device_is_a_noop() {
    let room: RoomId = [7; 16];
    let mut relay = Relay::new(config());
    let (tx_a, _rx_a) = unbounded_channel();
    relay.join(room, id(1), mac(1), tx_a);

    assert!(!relay.deliver(room, id(1), id(9), vec![0]));
    assert!(relay.devices(room).iter().all(|d| d.id != id(9)));
}

#[test]
fn join_announces_the_new_device_to_others() {
    let room: RoomId = [7; 16];
    let mut relay = Relay::new(config());
    let (tx_a, mut rx_a) = unbounded_channel();
    let (tx_b, _rx_b) = unbounded_channel();

    relay.join(room, id(1), mac(1), tx_a);
    let _ = drain(&mut rx_a); // discard A's Joined

    // B joins: A should receive an updated device list naming both devices.
    let outcome = relay.join(room, id(2), mac(2), tx_b);
    assert_eq!(outcome.to_others.len(), 1, "A should be notified");
    for (peer_tx, frame) in outcome.to_others {
        peer_tx.send(frame).unwrap();
    }

    match &drain(&mut rx_a)[0] {
        ServerFrame::Devices { devices } => {
            assert_eq!(devices.len(), 2);
            assert!(devices.iter().any(|d| d.id == id(2)));
        }
        other => panic!("expected Devices, got {other:?}"),
    }
}

#[test]
fn offline_devices_are_not_announced() {
    let room: RoomId = [7; 16];
    let mut relay = Relay::new(config());
    let (tx_a, _rx_a) = unbounded_channel();
    let (tx_b, _rx_b) = unbounded_channel();

    relay.join(room, id(1), mac(1), tx_a);
    relay.join(room, id(2), mac(2), tx_b);
    relay.disconnect(room, id(1));

    let (tx_c, _rx_c) = unbounded_channel();
    let outcome = relay.join(room, id(3), mac(3), tx_c);
    // Only B is live, so only B gets the update.
    assert_eq!(outcome.to_others.len(), 1);
}
