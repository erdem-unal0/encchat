//! In-room device registry and message routing.
//!
//! `Relay` is pure state: it turns client intents into a list of frames to send
//! to specific devices. It performs no I/O itself, which makes it straightforward
//! to test without a network.

use std::collections::HashMap;
use std::time::Instant;

use encchat_core::{encode, DeviceEntry, DeviceId, Mac, RoomId, ServerFrame};
use tokio::sync::mpsc::UnboundedSender;

use crate::mailbox::{Mailbox, MailboxConfig};

/// A live (or remembered) device inside a room.
struct DeviceState {
    mac: Mac,
    /// Present while the device has an open connection.
    tx: Option<UnboundedSender<Vec<u8>>>,
    mailbox: Mailbox,
}

/// Frames produced by a join: those for the joining device, and an updated
/// device list to announce to everyone already connected.
pub struct JoinOutcome {
    /// `Joined` plus any messages queued while the device was offline.
    pub to_self: Vec<Vec<u8>>,
    /// `Devices` updates for the other live connections, so they learn about
    /// the newcomer and can start a handshake.
    pub to_others: Vec<(UnboundedSender<Vec<u8>>, Vec<u8>)>,
}

/// All rooms and their devices.
pub struct Relay {
    rooms: HashMap<RoomId, HashMap<DeviceId, DeviceState>>,
    config: MailboxConfig,
}

impl Relay {
    pub fn new(config: MailboxConfig) -> Self {
        Self {
            rooms: HashMap::new(),
            config,
        }
    }

    /// Register (or re-register) a device and compute the frames to send.
    ///
    /// The `mac` is stored and echoed to peers but never validated here — the
    /// relay cannot check it, and peers verify it themselves.
    pub fn join(
        &mut self,
        room: RoomId,
        device: DeviceId,
        mac: Mac,
        tx: UnboundedSender<Vec<u8>>,
    ) -> JoinOutcome {
        let config = self.config;
        let devices = self.rooms.entry(room).or_default();

        {
            let entry = devices.entry(device).or_insert_with(|| DeviceState {
                mac,
                tx: None,
                mailbox: Mailbox::new(config),
            });
            entry.mac = mac;
            entry.tx = Some(tx);
        }

        let entries: Vec<DeviceEntry> = devices
            .iter()
            .map(|(id, state)| DeviceEntry { id: *id, mac: state.mac })
            .collect();

        let mut to_self = vec![encode(&ServerFrame::Joined {
            room,
            devices: entries.clone(),
        })
        .expect("Joined frame is always encodable")];

        if let Some(entry) = devices.get_mut(&device) {
            for queued in entry.mailbox.drain(Instant::now()) {
                to_self.push(
                    encode(&ServerFrame::Deliver {
                        from: queued.from,
                        payload: queued.payload,
                    })
                    .expect("Deliver frame is always encodable"),
                );
            }
        }

        let devices_frame = encode(&ServerFrame::Devices { devices: entries })
            .expect("Devices frame is always encodable");
        let mut to_others = Vec::new();
        for (id, state) in devices.iter() {
            if *id == device {
                continue;
            }
            if let Some(peer_tx) = &state.tx {
                to_others.push((peer_tx.clone(), devices_frame.clone()));
            }
        }

        JoinOutcome { to_self, to_others }
    }

    /// Snapshot of every device registered in a room.
    pub fn devices(&self, room: RoomId) -> Vec<DeviceEntry> {
        self.rooms
            .get(&room)
            .map(|devices| {
                devices
                    .iter()
                    .map(|(id, state)| DeviceEntry { id: *id, mac: state.mac })
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Deliver a payload to `to`, or queue it if that device is offline.
    ///
    /// Returns `true` if the payload was handed to a live connection.
    pub fn deliver(&mut self, room: RoomId, from: DeviceId, to: DeviceId, payload: Vec<u8>) -> bool {
        let now = Instant::now();
        let Some(devices) = self.rooms.get_mut(&room) else {
            return false;
        };
        let Some(target) = devices.get_mut(&to) else {
            // Relaying to a device that never registered: nothing to do.
            return false;
        };

        if let Some(tx) = &target.tx {
            let frame = encode(&ServerFrame::Deliver {
                from,
                payload: payload.clone(),
            })
            .expect("Deliver frame is always encodable");
            if tx.send(frame).is_ok() {
                return true;
            }
        }
        target.mailbox.push(from, payload, now);
        false
    }

    /// Forget a device's live connection but keep its registration so peers can
    /// still fan out to it while it is offline.
    pub fn disconnect(&mut self, room: RoomId, device: DeviceId) {
        if let Some(devices) = self.rooms.get_mut(&room) {
            if let Some(state) = devices.get_mut(&device) {
                state.tx = None;
            }
        }
    }
}
