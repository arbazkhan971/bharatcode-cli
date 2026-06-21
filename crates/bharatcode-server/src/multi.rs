//! Opt-in headless multi-session manager for `bharatcode serve --multi`.
//!
//! By default `bharatcode serve` hosts a single agent session and its behaviour
//! is byte-identical to the legacy single-session path: this module is only
//! reached when the operator passes `--multi`. When enabled, the serve path
//! stands up a [`MultiSessionRegistry`] — a bounded, in-process map of named
//! agent sessions keyed by an opaque session id — so one headless process can
//! host and route many concurrent sessions.
//!
//! The cap is read from the `BHARATCODE_MAX_SESSIONS` environment variable and
//! **defaults to `1`**, which keeps the portable-default build's behaviour
//! unchanged (a single slot, exactly like the legacy path) even when `--multi`
//! is supplied without tuning. Registering past the cap fails with
//! [`RegisterError::CapReached`] rather than silently evicting a live session.
//!
//! The registry is intentionally dependency-light (`std` + `chrono`, plus the
//! sibling [`SessionHandle`]). That lets the CLI crate `#[path]`-include this
//! module at its real `serve --multi` call site without pulling in the rest of
//! the server's dependency graph.

use std::collections::HashMap;
use std::sync::RwLock;

#[path = "multi_session_handle.rs"]
mod multi_session_handle;

pub use multi_session_handle::SessionHandle;

/// Environment key capping how many sessions a single headless process hosts.
///
/// Unset / unparseable => [`DEFAULT_MAX_SESSIONS`] (legacy single-session).
pub const MAX_SESSIONS_ENV: &str = "BHARATCODE_MAX_SESSIONS";

/// Default session cap when `BHARATCODE_MAX_SESSIONS` is unset.
///
/// `1` preserves single-session behaviour for the portable-default build.
pub const DEFAULT_MAX_SESSIONS: usize = 1;

/// Why a [`MultiSessionRegistry::register`] call was rejected.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RegisterError {
    /// The registry already holds `max` live sessions.
    CapReached {
        /// The configured cap that was hit.
        max: usize,
    },
    /// A session was already registered under this id.
    Duplicate {
        /// The id that collided.
        id: String,
    },
}

impl std::fmt::Display for RegisterError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RegisterError::CapReached { max } => {
                write!(f, "session cap reached ({max} active)")
            }
            RegisterError::Duplicate { id } => {
                write!(f, "session id already registered: {id}")
            }
        }
    }
}

impl std::error::Error for RegisterError {}

/// A bounded, in-process registry of named agent sessions.
///
/// Sessions are keyed by an opaque id; the registry enforces a hard cap on the
/// number of concurrently-hosted sessions (see [`MAX_SESSIONS_ENV`]).
pub struct MultiSessionRegistry {
    sessions: RwLock<HashMap<String, SessionHandle>>,
    max: usize,
}

impl MultiSessionRegistry {
    /// Build a registry with an explicit cap. A cap of `0` is clamped up to `1`
    /// so the registry can always hold at least one session.
    pub fn new(max: usize) -> Self {
        Self {
            sessions: RwLock::new(HashMap::new()),
            max: max.max(1),
        }
    }

    /// Build a registry sized from `BHARATCODE_MAX_SESSIONS`, defaulting to
    /// [`DEFAULT_MAX_SESSIONS`] (single-session) when the var is absent or not a
    /// positive integer.
    pub fn from_env() -> Self {
        let max = std::env::var(MAX_SESSIONS_ENV)
            .ok()
            .and_then(|raw| raw.trim().parse::<usize>().ok())
            .filter(|n| *n >= 1)
            .unwrap_or(DEFAULT_MAX_SESSIONS);
        Self::new(max)
    }

    /// The configured session cap.
    pub fn max_sessions(&self) -> usize {
        self.max
    }

    /// Register a new session under `id`, returning a clonable handle to its
    /// slot.
    ///
    /// Fails with [`RegisterError::CapReached`] if the registry is already at
    /// its cap, or [`RegisterError::Duplicate`] if `id` is already present.
    pub fn register(&self, id: impl Into<String>) -> Result<SessionHandle, RegisterError> {
        let id = id.into();
        let mut sessions = self.sessions.write().expect("registry lock poisoned");
        if sessions.contains_key(&id) {
            return Err(RegisterError::Duplicate { id });
        }
        if sessions.len() >= self.max {
            return Err(RegisterError::CapReached { max: self.max });
        }
        let handle = SessionHandle::new(id.clone());
        sessions.insert(id, handle.clone());
        Ok(handle)
    }

    /// Look up a live session by id, recording an access on hit.
    pub fn get(&self, id: &str) -> Option<SessionHandle> {
        let sessions = self.sessions.read().expect("registry lock poisoned");
        let handle = sessions.get(id)?;
        handle.touch();
        Some(handle.clone())
    }

    /// Drop the session registered under `id`, freeing its slot.
    ///
    /// Returns `true` if a session was removed, `false` if none was registered.
    pub fn drop_session(&self, id: &str) -> bool {
        let mut sessions = self.sessions.write().expect("registry lock poisoned");
        sessions.remove(id).is_some()
    }

    /// Number of currently-registered sessions.
    pub fn active_count(&self) -> usize {
        self.sessions.read().expect("registry lock poisoned").len()
    }

    /// Whether the registry is at its cap and would reject a fresh `register`.
    pub fn is_full(&self) -> bool {
        self.active_count() >= self.max
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registers_up_to_cap_then_reports_cap_reached() {
        let reg = MultiSessionRegistry::new(3);
        assert_eq!(reg.max_sessions(), 3);
        assert_eq!(reg.active_count(), 0);

        let a = reg.register("a").expect("first slot");
        let b = reg.register("b").expect("second slot");
        let c = reg.register("c").expect("third slot");
        assert_eq!(reg.active_count(), 3);
        assert!(reg.is_full());

        // Distinct slots: each id maps to its own handle.
        assert_eq!(a.id(), "a");
        assert_eq!(b.id(), "b");
        assert_eq!(c.id(), "c");

        // Over-cap registration is rejected, count is unchanged.
        let over = reg.register("d");
        assert_eq!(over, Err(RegisterError::CapReached { max: 3 }));
        assert_eq!(reg.active_count(), 3);
    }

    #[test]
    fn drop_session_frees_a_slot() {
        let reg = MultiSessionRegistry::new(2);
        reg.register("a").expect("slot a");
        reg.register("b").expect("slot b");
        assert_eq!(reg.active_count(), 2);
        assert!(reg.register("c").is_err());

        assert!(reg.drop_session("a"));
        assert_eq!(reg.active_count(), 1);
        assert!(!reg.is_full());

        // The freed slot can be reused.
        let reused = reg.register("c").expect("reused slot");
        assert_eq!(reused.id(), "c");
        assert_eq!(reg.active_count(), 2);

        // Dropping an unknown id is a no-op.
        assert!(!reg.drop_session("zzz"));
        assert_eq!(reg.active_count(), 2);
    }

    #[test]
    fn duplicate_ids_are_rejected_without_consuming_a_slot() {
        let reg = MultiSessionRegistry::new(4);
        reg.register("dup").expect("first");
        let again = reg.register("dup");
        assert_eq!(
            again,
            Err(RegisterError::Duplicate {
                id: "dup".to_string()
            })
        );
        assert_eq!(reg.active_count(), 1);
    }

    #[test]
    fn get_returns_handle_and_records_access() {
        let reg = MultiSessionRegistry::new(2);
        let h = reg.register("s").expect("slot");
        assert_eq!(h.access_count(), 0);

        let got = reg.get("s").expect("present");
        assert_eq!(got.id(), "s");
        assert_eq!(got.access_count(), 1);
        // Original handle observes the shared tally too.
        assert_eq!(h.access_count(), 1);

        assert!(reg.get("missing").is_none());
    }

    #[test]
    fn zero_cap_is_clamped_to_one() {
        let reg = MultiSessionRegistry::new(0);
        assert_eq!(reg.max_sessions(), 1);
        reg.register("only").expect("one slot fits");
        assert!(reg.register("nope").is_err());
    }
}
