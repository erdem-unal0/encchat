//! In-memory, per-device TTL mailbox for devices that are currently offline.
//!
//! The relay is blind: the payloads stored here are already end-to-end
//! encrypted by the clients, and the server never inspects them.

use std::collections::VecDeque;
use std::time::{Duration, Instant};

use encchat_core::DeviceId;

/// Bounds for a single device mailbox.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MailboxConfig {
    /// Maximum number of queued messages. When full, the oldest entry is
    /// dropped to make room for the newest one.
    pub max_entries: usize,
    /// How long a queued message may wait before it is discarded.
    pub ttl: Duration,
}

impl Default for MailboxConfig {
    fn default() -> Self {
        Self {
            max_entries: 512,
            ttl: Duration::from_secs(300),
        }
    }
}

/// One message waiting for an offline device.
#[derive(Clone, Debug)]
pub struct QueuedMessage {
    pub from: DeviceId,
    pub payload: Vec<u8>,
    pub queued_at: Instant,
}

/// FIFO mailbox with a size cap and a time-to-live.
#[derive(Debug)]
pub struct Mailbox {
    config: MailboxConfig,
    queue: VecDeque<QueuedMessage>,
}

impl Mailbox {
    pub fn new(config: MailboxConfig) -> Self {
        Self {
            config,
            queue: VecDeque::new(),
        }
    }

    /// Drop entries that have outlived [`MailboxConfig::ttl`].
    fn prune(&mut self, now: Instant) {
        if self.config.ttl.is_zero() {
            self.queue.clear();
            return;
        }
        while let Some(front) = self.queue.front() {
            if now.saturating_duration_since(front.queued_at) >= self.config.ttl {
                self.queue.pop_front();
            } else {
                break;
            }
        }
    }

    /// Append a message, expiring stale entries and dropping the oldest one
    /// when the mailbox is already at capacity.
    pub fn push(&mut self, from: DeviceId, payload: Vec<u8>, now: Instant) {
        self.prune(now);
        if self.config.max_entries == 0 {
            return;
        }
        while self.queue.len() >= self.config.max_entries {
            self.queue.pop_front();
        }
        self.queue.push_back(QueuedMessage {
            from,
            payload,
            queued_at: now,
        });
    }

    /// Restore messages at the front of the queue, preserving their original
    /// timestamps. Used when a flush fails partway through.
    pub fn restore_front(&mut self, messages: Vec<QueuedMessage>, now: Instant) {
        self.prune(now);
        if self.config.max_entries == 0 {
            return;
        }
        for message in messages.into_iter().rev() {
            self.queue.push_front(message);
        }
        while self.queue.len() > self.config.max_entries {
            self.queue.pop_back();
        }
    }

    /// Remove and return every live message, oldest first.
    pub fn drain(&mut self, now: Instant) -> Vec<QueuedMessage> {
        self.prune(now);
        self.queue.drain(..).collect()
    }

    /// Number of live messages, after expiring stale ones.
    pub fn len(&mut self, now: Instant) -> usize {
        self.prune(now);
        self.queue.len()
    }

    pub fn is_empty(&mut self, now: Instant) -> bool {
        self.len(now) == 0
    }

    pub fn config(&self) -> MailboxConfig {
        self.config
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config(max_entries: usize, ttl_secs: u64) -> MailboxConfig {
        MailboxConfig {
            max_entries,
            ttl: Duration::from_secs(ttl_secs),
        }
    }

    fn msg(from: u8, body: u8, at: Instant) -> QueuedMessage {
        QueuedMessage {
            from: [from; 16],
            payload: vec![body],
            queued_at: at,
        }
    }

    #[test]
    fn drains_in_fifo_order_and_empties() {
        let t0 = Instant::now();
        let mut mailbox = Mailbox::new(config(8, 60));
        mailbox.push([1; 16], vec![1], t0);
        mailbox.push([2; 16], vec![2], t0);
        mailbox.push([3; 16], vec![3], t0);

        let drained = mailbox.drain(t0);
        let payloads: Vec<Vec<u8>> = drained.into_iter().map(|m| m.payload).collect();
        assert_eq!(payloads, vec![vec![1], vec![2], vec![3]]);
        assert!(mailbox.is_empty(t0));
    }

    #[test]
    fn entry_expires_at_ttl_boundary() {
        let t0 = Instant::now();
        let mut mailbox = Mailbox::new(config(8, 60));
        mailbox.push([1; 16], vec![1], t0);

        // Just before the TTL: still there.
        let before = t0 + Duration::from_secs(59);
        assert_eq!(mailbox.len(before), 1);

        // Exactly at the TTL: expired.
        let at = t0 + Duration::from_secs(60);
        assert_eq!(mailbox.len(at), 0);
        assert!(mailbox.drain(at).is_empty());
    }

    #[test]
    fn adding_after_expiry_does_not_resurrect_old_entries() {
        let t0 = Instant::now();
        let mut mailbox = Mailbox::new(config(8, 30));
        mailbox.push([1; 16], vec![1], t0);

        let t1 = t0 + Duration::from_secs(31);
        mailbox.push([2; 16], vec![2], t1);

        let drained = mailbox.drain(t1);
        assert_eq!(drained.len(), 1);
        assert_eq!(drained[0].payload, vec![2]);
    }

    #[test]
    fn cap_drops_oldest_entries() {
        let t0 = Instant::now();
        let mut mailbox = Mailbox::new(config(3, 60));
        for body in 0..5u8 {
            mailbox.push([body; 16], vec![body], t0);
        }

        let drained = mailbox.drain(t0);
        let payloads: Vec<Vec<u8>> = drained.into_iter().map(|m| m.payload).collect();
        assert_eq!(payloads, vec![vec![2], vec![3], vec![4]]);
    }

    #[test]
    fn zero_capacity_queues_nothing() {
        let t0 = Instant::now();
        let mut mailbox = Mailbox::new(config(0, 60));
        mailbox.push([1; 16], vec![1], t0);
        assert!(mailbox.is_empty(t0));
    }

    #[test]
    fn zero_ttl_expires_immediately() {
        let t0 = Instant::now();
        let mut mailbox = Mailbox::new(config(8, 0));
        mailbox.push([1; 16], vec![1], t0);
        assert!(mailbox.is_empty(t0));
    }

    #[test]
    fn restore_front_preserves_order() {
        let t0 = Instant::now();
        let mut mailbox = Mailbox::new(config(8, 60));
        mailbox.push([2; 16], vec![2], t0);
        mailbox.restore_front(vec![msg(1, 1, t0), msg(11, 11, t0)], t0);

        let payloads: Vec<Vec<u8>> = mailbox
            .drain(t0)
            .into_iter()
            .map(|m| m.payload)
            .collect();
        assert_eq!(payloads, vec![vec![1], vec![11], vec![2]]);
    }
}
