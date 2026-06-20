//! Per-session bookkeeping for the opt-in headless multi-session manager.
//!
//! A [`SessionHandle`] is the value stored in the multi-session registry for
//! each live agent session. It is deliberately lightweight and free of any
//! server-internal types so the module can be `#[path]`-included from the CLI
//! crate (the real `bharatcode serve --multi` call site) without dragging in
//! the whole server dependency graph. It carries only what a router needs to
//! distinguish, age out, and account for concurrent sessions: the opaque
//! session id, a creation timestamp, and a monotonic access counter.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use chrono::{DateTime, Utc};

/// Opaque, lightweight record describing one hosted session slot.
///
/// Cloning a handle is cheap: the access counter is shared (`Arc<AtomicU64>`)
/// so every clone observes the same usage tally, which lets the registry track
/// recency without holding its map locked.
#[derive(Clone, Debug)]
pub struct SessionHandle {
    id: String,
    created_at: DateTime<Utc>,
    accesses: Arc<AtomicU64>,
}

impl SessionHandle {
    /// Create a handle for a freshly registered session id.
    pub fn new(id: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            created_at: Utc::now(),
            accesses: Arc::new(AtomicU64::new(0)),
        }
    }

    /// The opaque session id this handle was registered under.
    pub fn id(&self) -> &str {
        &self.id
    }

    /// Wall-clock instant the session slot was created.
    pub fn created_at(&self) -> DateTime<Utc> {
        self.created_at
    }

    /// Record (and return) one more access against this session.
    ///
    /// Returns the access count *after* this touch, so the first touch yields
    /// `1`. Used by the registry to reason about recency.
    pub fn touch(&self) -> u64 {
        self.accesses.fetch_add(1, Ordering::Relaxed) + 1
    }

    /// Current access tally for this session slot.
    pub fn access_count(&self) -> u64 {
        self.accesses.load(Ordering::Relaxed)
    }
}

#[cfg(test)]
mod tests {
    use super::SessionHandle;

    #[test]
    fn handle_tracks_id_and_accesses() {
        let h = SessionHandle::new("alpha");
        assert_eq!(h.id(), "alpha");
        assert_eq!(h.access_count(), 0);
        assert_eq!(h.touch(), 1);
        assert_eq!(h.touch(), 2);
        assert_eq!(h.access_count(), 2);
    }

    #[test]
    fn clones_share_access_tally() {
        let h = SessionHandle::new("beta");
        let c = h.clone();
        h.touch();
        c.touch();
        assert_eq!(h.access_count(), 2);
        assert_eq!(c.access_count(), 2);
    }
}
