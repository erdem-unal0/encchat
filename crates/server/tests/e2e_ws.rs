//! End-to-end test: two clients, a real relay, real WebSockets.
//!
//! This exercises the full stack — `ClientCore` (handshake + Double Ratchet),
//! postcard framing, the axum WebSocket front end and the routing state — the
//! same way the CLI and web app do.

use std::time::Duration;

use encchat_core::{
    decode, encode, Action, ClientCore, ClientFrame, DeviceId, PeerFrame, Psk, ServerFrame,
};
use futures_util::{SinkExt, StreamExt};
use tokio::net::TcpStream;
use tokio_tungstenite::{tungstenite::Message, MaybeTlsStream, WebSocketStream};

type Ws = WebSocketStream<MaybeTlsStream<TcpStream>>;

struct Peer {
    sink: futures_util::stream::SplitSink<Ws, Message>,
    stream: futures_util::stream::SplitStream<Ws>,
    core: ClientCore,
    peers: Vec<DeviceId>,
    received: Vec<String>,
}

impl Peer {
    async fn connect(url: &str, psk: &Psk) -> Self {
        let (ws, _) = tokio_tungstenite::connect_async(url)
            .await
            .expect("websocket connects");
        let (sink, stream) = ws.split();
        let mut peer = Peer {
            sink,
            stream,
            core: ClientCore::new(psk.clone()).expect("core"),
            peers: Vec::new(),
            received: Vec::new(),
        };
        peer.send(encode(&peer.core.join_frame()).unwrap()).await;
        peer
    }

    async fn send(&mut self, bytes: Vec<u8>) {
        self.sink
            .send(Message::Binary(bytes.into()))
            .await
            .expect("send");
    }

    async fn relay(&mut self, to: DeviceId, frame: &PeerFrame) {
        let payload = encode(frame).unwrap();
        self.send(encode(&ClientFrame::Relay { to, payload }).unwrap())
            .await;
    }

    /// Read one server frame, react to it, and send any resulting frames.
    async fn step(&mut self) {
        let message = self
            .stream
            .next()
            .await
            .expect("stream open")
            .expect("frame");
        let bytes = match message {
            Message::Binary(bytes) => bytes,
            other => panic!("unexpected frame: {other:?}"),
        };
        let frame: ServerFrame = decode(&bytes).expect("server frame decodes");

        match frame {
            ServerFrame::Joined { devices, .. } | ServerFrame::Devices { devices } => {
                let me = self.core.device_id();
                for entry in &devices {
                    if entry.id == me || self.peers.contains(&entry.id) {
                        continue;
                    }
                    assert!(self.core.verify_device(entry), "device MAC must verify");
                    self.peers.push(entry.id);
                    if self.core.should_initiate(entry.id) {
                        let frame = self.core.begin_handshake(entry.id).unwrap();
                        self.relay(entry.id, &frame).await;
                    }
                }
            }
            ServerFrame::Deliver { from, payload } => {
                let peer_frame: PeerFrame = decode(&payload).expect("peer frame decodes");
                for action in self.core.handle_frame(from, peer_frame).unwrap() {
                    match action {
                        Action::Send { to, frame } => self.relay(to, &frame).await,
                        Action::Received { body, .. } => self.received.push(body),
                        Action::HandshakeComplete { with } => {
                            if !self.peers.contains(&with) {
                                self.peers.push(with);
                            }
                        }
                        Action::Notice(_) => {}
                    }
                }
            }
            ServerFrame::Error { code, message } => panic!("server error {code}: {message}"),
            ServerFrame::Pong => {}
        }
    }

    async fn send_text(&mut self, body: &str) {
        let peers = self.peers.clone();
        let frames = self
            .core
            .send_text_to_all(&peers, body, 0)
            .expect("has an established session");
        for (to, frame) in frames {
            self.relay(to, &frame).await;
        }
    }
}

/// Drive both peers until `done` holds, or fail after a timeout.
async fn pump(a: &mut Peer, b: &mut Peer, mut done: impl FnMut(&Peer, &Peer) -> bool) {
    for _ in 0..200 {
        if done(a, b) {
            return;
        }
        let step = tokio::time::timeout(Duration::from_secs(5), async {
            tokio::select! {
                _ = a.step() => {}
                _ = b.step() => {}
            }
        });
        step.await.expect("no progress within the timeout");
    }
    panic!("peers did not converge");
}

async fn spawn_relay() -> String {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let app = encchat_server::build_router(encchat_server::default_config(60, 32), 262_144);
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    format!("ws://{addr}/ws")
}

#[tokio::test]
async fn two_clients_handshake_and_chat_through_the_relay() {
    let url = spawn_relay().await;
    let psk = Psk::generate().unwrap();

    let mut alice = Peer::connect(&url, &psk).await;
    let mut bob = Peer::connect(&url, &psk).await;

    // Both devices discover each other and complete exactly one handshake.
    pump(&mut alice, &mut bob, |a, b| {
        a.core.session_count() == 1 && b.core.session_count() == 1
    })
    .await;
    assert_eq!(alice.peers.len(), 1);
    assert_eq!(bob.peers.len(), 1);

    // Alice -> Bob.
    alice.send_text("hello through the relay").await;
    pump(&mut alice, &mut bob, |_, b| {
        b.received.iter().any(|m| m == "hello through the relay")
    })
    .await;

    // Bob -> Alice (forces a DH ratchet).
    bob.send_text("and back").await;
    pump(&mut alice, &mut bob, |a, _| {
        a.received.iter().any(|m| m == "and back")
    })
    .await;
}

#[tokio::test]
async fn a_wrong_psk_peer_lands_in_a_different_room() {
    let url = spawn_relay().await;
    let good = Psk::generate().unwrap();
    let bad = Psk::from_bytes([0x42; 32]);

    let mut alice = Peer::connect(&url, &good).await;
    let mut bob = Peer::connect(&url, &bad).await;

    // Each sees only itself: the room ID is derived from the key, so a peer
    // with a different key is in a different room and never appears.
    alice.step().await;
    bob.step().await;

    assert!(alice.peers.is_empty(), "wrong-key peer must not be visible");
    assert!(bob.peers.is_empty(), "wrong-key peer must not be visible");
    assert_eq!(alice.core.session_count(), 0);
    assert_eq!(bob.core.session_count(), 0);
}
