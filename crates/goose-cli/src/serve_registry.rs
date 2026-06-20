//! Bounded multi-session registry for the headless `bharatcode serve` path.
//!
//! `bharatcode serve` hosts agent sessions over an ACP endpoint. By default it
//! behaves exactly as before for a single session; this module adds a real,
//! concurrency-capped registry so one headless process can host several
//! concurrent sessions, each identified by an opaque `session_id`, without ever
//! exceeding a configurable ceiling.
//!
//! The registry tracks `{ id -> SessionSlot }` where each slot records its
//! creation instant in both UTC and India Standard Time (UTC+05:30, the wall
//! clock surfaced to operators here) plus an optional human label. [`admit`]
//! reserves a slot and fails with a typed [`RegistryError::RegistryFull`] once
//! the cap is reached; [`release`] frees a slot so a fresh `admit` can reuse the
//! headroom. The live occupancy is always available via [`active_count`].
//!
//! It is intentionally pure (`std` + `chrono`): no network, no server-crate
//! dependency, so the portable-default build keeps compiling everywhere.
//!
//! Tuning:
//!   * `BHARATCODE_MAX_SESSIONS`  integer, clamped to `1..=256` (default `8`).
//!
//! [`admit`]: SessionRegistry::admit
//! [`release`]: SessionRegistry::release
//! [`active_count`]: SessionRegistry::active_count

use std::collections::HashMap;
use std::fmt;
use std::sync::Mutex;

use chrono::{DateTime, FixedOffset, Utc};

/// Environment key that caps how many concurrent sessions a single headless
/// `serve` process will host. Absent / unparseable => [`DEFAULT_MAX_SESSIONS`].
pub const MAX_SESSIONS_ENV: &str = "BHARATCODE_MAX_SESSIONS";

/// Lower bound for the concurrency cap (a registry must admit at least one).
const MIN_SESSIONS: usize = 1;
/// Upper bound for the concurrency cap (guards against absurd / hostile values).
const MAX_SESSIONS_CEILING: usize = 256;
/// Default cap when `BHARATCODE_MAX_SESSIONS` is unset or not a valid integer.
const DEFAULT_MAX_SESSIONS: usize = 8;

/// India Standard Time (UTC+05:30), the wall clock surfaced to operators.
fn ist_offset() -> FixedOffset {
    FixedOffset::east_opt(5 * 3600 + 30 * 60).expect("IST (+05:30) is a valid fixed offset")
}

/// Why an [`SessionRegistry::admit`] call was rejected.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RegistryError {
    /// The registry is already at its concurrency cap and cannot admit more.
    RegistryFull {
        /// The configured maximum number of concurrent sessions.
        cap: usize,
    },
    /// A session was already admitted under this id.
    DuplicateId {
        /// The conflicting session id.
        id: String,
    },
}

impl fmt::Display for RegistryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RegistryError::RegistryFull { cap } => {
                write!(f, "session registry is full (cap {cap})")
            }
            RegistryError::DuplicateId { id } => {
                write!(f, "session id already admitted: {id}")
            }
        }
    }
}

impl std::error::Error for RegistryError {}

/// One admitted session: when it was created and an optional human label.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionSlot {
    /// Creation instant in UTC.
    pub created_at_utc: DateTime<Utc>,
    /// Same instant rendered into India Standard Time (UTC+05:30).
    pub created_at_ist: DateTime<FixedOffset>,
    /// Optional operator-facing label (free-form; never required).
    pub label: Option<String>,
}

impl SessionSlot {
    fn now(label: Option<String>) -> Self {
        let created_at_utc = Utc::now();
        Self {
            created_at_utc,
            created_at_ist: created_at_utc.with_timezone(&ist_offset()),
            label,
        }
    }
}

/// A bounded, in-process registry of concurrently-hosted sessions keyed by an
/// opaque `session_id`. Admission is capped at [`SessionRegistry::capacity`].
#[derive(Debug)]
pub struct SessionRegistry {
    /// Maximum number of concurrently-admitted sessions.
    cap: usize,
    /// Currently-admitted sessions, keyed by id.
    slots: Mutex<HashMap<String, SessionSlot>>,
}

impl SessionRegistry {
    /// Build a registry with an explicit cap, clamped to `1..=256`.
    pub fn new(cap: usize) -> Self {
        Self {
            cap: cap.clamp(MIN_SESSIONS, MAX_SESSIONS_CEILING),
            slots: Mutex::new(HashMap::new()),
        }
    }

    /// Build a registry sized from `BHARATCODE_MAX_SESSIONS`, defaulting to
    /// [`DEFAULT_MAX_SESSIONS`] (`8`) when the var is absent or not a valid
    /// integer, and clamping any value into `1..=256`.
    ///
    /// The raw environment variable is read first (mirroring `is_enabled` in the
    /// memory store), so an explicit env override always wins.
    pub fn from_env() -> Self {
        let cap = std::env::var(MAX_SESSIONS_ENV)
            .ok()
            .and_then(|raw| raw.trim().parse::<usize>().ok())
            .unwrap_or(DEFAULT_MAX_SESSIONS);
        Self::new(cap)
    }

    /// The configured concurrency ceiling (already clamped).
    pub fn capacity(&self) -> usize {
        self.cap
    }

    /// Number of sessions currently admitted (live occupancy).
    pub fn active_count(&self) -> usize {
        self.slots.lock().expect("registry mutex poisoned").len()
    }

    /// Whether the registry is at its cap and would reject a fresh [`admit`].
    ///
    /// [`admit`]: SessionRegistry::admit
    pub fn is_full(&self) -> bool {
        self.active_count() >= self.cap
    }

    /// Admit a new session under `id`, recording its creation timestamps and an
    /// optional `label`.
    ///
    /// Returns [`RegistryError::RegistryFull`] once the cap is reached, or
    /// [`RegistryError::DuplicateId`] if `id` is already admitted.
    pub fn admit(
        &self,
        id: impl Into<String>,
        label: Option<String>,
    ) -> Result<SessionSlot, RegistryError> {
        let id = id.into();
        let mut slots = self.slots.lock().expect("registry mutex poisoned");
        if slots.contains_key(&id) {
            return Err(RegistryError::DuplicateId { id });
        }
        if slots.len() >= self.cap {
            return Err(RegistryError::RegistryFull { cap: self.cap });
        }
        let slot = SessionSlot::now(label);
        slots.insert(id, slot.clone());
        Ok(slot)
    }

    /// Release the session admitted under `id`, freeing its slot.
    ///
    /// Returns `true` if a session was removed, `false` if none was admitted.
    pub fn release(&self, id: &str) -> bool {
        self.slots
            .lock()
            .expect("registry mutex poisoned")
            .remove(id)
            .is_some()
    }

    /// One-line, operator-facing summary of the registry's capacity, suitable
    /// for logging at serve startup. Contains the cap and carries no upstream
    /// project branding.
    pub fn capacity_line(&self) -> String {
        format!(
            "Session registry ready: up to {} concurrent session(s) (env {})",
            self.cap, MAX_SESSIONS_ENV
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Serialises the env-var-touching tests; `set_var`/`remove_var` mutate
    /// process-global state and would otherwise race under the test runner.
    static ENV_GUARD: Mutex<()> = Mutex::new(());

    #[test]
    fn admit_up_to_cap_then_registry_full() {
        let reg = SessionRegistry::new(3);
        assert_eq!(reg.capacity(), 3);
        assert_eq!(reg.active_count(), 0);

        reg.admit("a", None).expect("first slot");
        assert_eq!(reg.active_count(), 1);
        reg.admit("b", Some("worker".into())).expect("second slot");
        reg.admit("c", None).expect("third slot");
        assert_eq!(reg.active_count(), 3);
        assert!(reg.is_full());

        // The (cap + 1)th admit is rejected with a typed RegistryFull.
        let over = reg.admit("d", None);
        assert_eq!(over, Err(RegistryError::RegistryFull { cap: 3 }));
        assert_eq!(
            reg.active_count(),
            3,
            "rejected admit must not grow the map"
        );
    }

    #[test]
    fn release_decrements_and_frees_a_slot_for_re_admit() {
        let reg = SessionRegistry::new(2);
        reg.admit("a", None).expect("slot a");
        reg.admit("b", None).expect("slot b");
        assert_eq!(reg.active_count(), 2);
        assert!(reg.admit("c", None).is_err(), "full registry rejects");

        assert!(reg.release("a"), "release removes an admitted slot");
        assert_eq!(reg.active_count(), 1);
        assert!(!reg.release("a"), "second release is a no-op");

        let reused = reg.admit("c", None);
        assert!(reused.is_ok(), "freed headroom allows a fresh admit");
        assert_eq!(reg.active_count(), 2);
    }

    #[test]
    fn duplicate_id_is_rejected() {
        let reg = SessionRegistry::new(4);
        reg.admit("dup", None).expect("first admit");
        assert_eq!(
            reg.admit("dup", None),
            Err(RegistryError::DuplicateId { id: "dup".into() })
        );
        assert_eq!(reg.active_count(), 1);
    }

    #[test]
    fn slot_records_utc_and_ist_offset() {
        let reg = SessionRegistry::new(1);
        let slot = reg.admit("x", Some("label".into())).expect("admit");
        assert_eq!(slot.label.as_deref(), Some("label"));
        // IST is UTC + 5h30m: the fixed offset must be exactly +19800 seconds.
        assert_eq!(
            slot.created_at_ist.offset().local_minus_utc(),
            5 * 3600 + 30 * 60
        );
        // Both stamps describe the same instant.
        assert_eq!(
            slot.created_at_ist.timestamp(),
            slot.created_at_utc.timestamp()
        );
    }

    #[test]
    fn from_env_defaults_and_clamps() {
        let _guard = ENV_GUARD.lock().unwrap();

        std::env::remove_var(MAX_SESSIONS_ENV);
        assert_eq!(
            SessionRegistry::from_env().capacity(),
            8,
            "unset => default 8"
        );

        std::env::set_var(MAX_SESSIONS_ENV, "9999");
        assert_eq!(
            SessionRegistry::from_env().capacity(),
            256,
            "bogus 9999 clamps down to 256"
        );

        std::env::set_var(MAX_SESSIONS_ENV, "0");
        assert_eq!(
            SessionRegistry::from_env().capacity(),
            1,
            "0 clamps up to 1"
        );

        std::env::set_var(MAX_SESSIONS_ENV, "32");
        assert_eq!(
            SessionRegistry::from_env().capacity(),
            32,
            "in-range value passes through"
        );

        std::env::set_var(MAX_SESSIONS_ENV, "not-a-number");
        assert_eq!(
            SessionRegistry::from_env().capacity(),
            8,
            "garbage falls back to default"
        );

        std::env::remove_var(MAX_SESSIONS_ENV);
    }

    #[test]
    fn new_clamps_explicit_cap() {
        assert_eq!(SessionRegistry::new(0).capacity(), 1);
        assert_eq!(SessionRegistry::new(9999).capacity(), 256);
        assert_eq!(SessionRegistry::new(8).capacity(), 8);
    }

    #[test]
    fn capacity_line_is_one_line_with_cap_and_no_upstream_branding() {
        let line = SessionRegistry::new(8).capacity_line();
        assert!(!line.contains('\n'), "capacity line must be a single line");
        assert!(line.contains('8'), "capacity line must mention the cap");
        assert!(
            !line.to_ascii_lowercase().contains("goose"),
            "no upstream project name"
        );
        assert!(!line.contains("Block"), "no upstream vendor name");
    }
}
