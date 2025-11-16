//! # Debouncer for notifications.
//!
//! Sometimes the client application may be reinstalled
//! while keeping the notification token.
//! In this case the same token is stored twice
//! for the same mailbox on a chatmail relay
//! and is notified twice for the same message.
//! Since it is not possible for the chatmail relay
//! to deduplicate the tokens in this case
//! as only the notification gateway
//! can decrypt them, notification gateway needs
//! to debounce notifications to the same token.

use std::cmp::Reverse;
use std::collections::{BinaryHeap, HashSet};
use std::sync::RwLock;
use std::time::{Duration, Instant};

#[derive(Default)]
pub(crate) struct Debouncer {
    state: RwLock<DebouncerState>,
}

#[derive(Default)]
struct DebouncerState {
    /// Set of recently notified tokens.
    ///
    /// The tokens are stored in plaintext,
    /// not hashed or encrypted.
    /// No token is stored for a long time anyway.
    tokens: HashSet<String>,

    /// Binary heap storing tokens
    /// sorted by the timestamp of the recent notifications.
    ///
    /// `Reverse` is used to turn max-heap into min-heap.
    heap: BinaryHeap<Reverse<(Instant, String)>>,
}

impl DebouncerState {
    /// Removes old entries for tokens that can be notified again.
    fn cleanup(&mut self, now: Instant) {
        loop {
            let Some(Reverse((timestamp, token))) = self.heap.pop() else {
                debug_assert!(self.tokens.is_empty());
                break;
            };

            if now.duration_since(timestamp) < Duration::from_secs(1) {
                self.heap.push(Reverse((timestamp, token)));
                break;
            }

            self.tokens.remove(&token);
        }
    }

    #[cfg(test)]
    fn is_debounced(&mut self, now: Instant, token: &String) -> bool {
        self.cleanup(now);
        self.tokens.contains(token)
    }

    fn notify(&mut self, now: Instant, token: String) -> bool {
        self.cleanup(now);
        let inserted = self.tokens.insert(token.clone());
        if inserted {
            self.heap.push(Reverse((now, token)));
        }
        !inserted
    }

    fn count(&self) -> usize {
        let res = self.tokens.len();
        debug_assert_eq!(res, self.heap.len());
        res
    }
}

impl Debouncer {
    /// Returns true if the token was notified recently
    /// and should not be notified again.
    #[cfg(test)]
    pub(crate) fn is_debounced(&self, now: Instant, token: &String) -> bool {
        let mut state = self.state.write().unwrap();
        state.is_debounced(now, token)
    }

    /// Returns true if notification should be sent,
    /// false if the token is currently debounced.
    pub(crate) fn notify(&self, now: Instant, token: String) -> bool {
        self.state.write().unwrap().notify(now, token)
    }

    /// Returns number of currently debounced notification tokens.
    ///
    /// This is used for metrics to display the size of the set.
    ///
    /// This function does not remove expired tokens.
    pub(crate) fn count(&self) -> usize {
        self.state.read().unwrap().count()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_debouncer() {
        let mut now = Instant::now();

        let debouncer = Debouncer::default();

        let token1 = "foobar".to_string();
        let token2 = "barbaz".to_string();

        assert!(!debouncer.is_debounced(now, &token1));
        assert!(!debouncer.is_debounced(now, &token2));
        assert_eq!(debouncer.count(), 0);

        debouncer.notify(now, token1.clone());

        assert!(debouncer.is_debounced(now, &token1));
        assert!(!debouncer.is_debounced(now, &token2));
        assert_eq!(debouncer.count(), 1);

        now += Duration::from_secs(5);

        assert!(!debouncer.is_debounced(now, &token1));
        assert!(!debouncer.is_debounced(now, &token2));
        assert_eq!(debouncer.count(), 0);
    }
}
