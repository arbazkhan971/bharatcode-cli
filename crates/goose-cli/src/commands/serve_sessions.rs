//! `bharatcode serve-sessions` — an opt-in, headless multi-session supervisor.
//!
//! A single BharatCode process holds an in-memory registry of named, isolated
//! agent sessions and exposes a tiny line-delimited JSON protocol over a
//! loopback TCP socket. A client opens a connection and sends one JSON command
//! per line (`Create`, `List`, `Route`, `Close`); each command is dispatched
//! against the [`SessionRegistry`], and a JSON response line is written back.
//! This lets a single instance drive several concurrent agent sessions without
//! standing up the full desktop server.
//!
//! Safety posture (default OFF, loopback-only):
//!   * The supervisor only starts when the user explicitly runs the subcommand.
//!   * The bind address MUST be loopback (`127.0.0.0/8` or `::1`) unless the
//!     `BHARATCODE_SERVE_BIND` env var explicitly whitelists the host. A
//!     non-loopback address is rejected with a hard error *before* the socket
//!     is ever bound.
//!   * With no client connected the process simply idles, and a `SIGINT`
//!     (Ctrl-C) shuts it down cleanly with exit code 0.
//!
//! Each routed prompt is run through the existing headless code path
//! ([`crate::session::CliSession::headless`], which wraps
//! `process_agent_response`), reusing [`crate::session::builder::build_session`]
//! to construct one isolated [`crate::session::CliSession`] per named session.

use std::collections::HashMap;
use std::net::IpAddr;
use std::sync::Arc;

use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::Mutex;

use crate::session::{build_session, CliSession, SessionBuilderConfig};

/// Options that drive the `serve-sessions` supervisor.
#[derive(Debug, Clone, Default)]
pub struct ServeSessionsOptions {
    /// Address to bind, e.g. `127.0.0.1:7878`. Defaults to `127.0.0.1:0`
    /// (an OS-chosen ephemeral loopback port) when `None`.
    pub addr: Option<String>,
    /// Optional cap on the number of concurrently registered sessions. `None`
    /// means unbounded.
    pub max_sessions: Option<usize>,
}

/// Default bind address when the caller passes no `--addr`. Loopback-only and an
/// ephemeral port so it never collides and never escapes the local host.
const DEFAULT_ADDR: &str = "127.0.0.1:0";

/// Env var that, when set, whitelists a single non-loopback host the supervisor
/// is permitted to bind. Unset (the default) keeps the supervisor loopback-only.
const BIND_WHITELIST_ENV: &str = "BHARATCODE_SERVE_BIND";

/// A single registered session: the live [`CliSession`] plus its stable id.
///
/// The handle owns the session behind nothing extra here; the registry as a
/// whole is guarded by a [`Mutex`] so routing is serialized per supervisor.
pub struct SessionHandle {
    /// Stable session id assigned by the session builder (isolation key).
    pub id: String,
    /// The live, headless agent session this handle routes prompts into.
    pub session: CliSession,
}

impl std::fmt::Debug for SessionHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SessionHandle")
            .field("id", &self.id)
            .finish_non_exhaustive()
    }
}

/// In-memory registry of named, isolated sessions, keyed by the client-supplied
/// name. Closing a session removes it; routing requires it to already exist.
#[derive(Default)]
pub struct SessionRegistry {
    /// Named sessions. The map key is the human name; the value carries the
    /// builder-assigned session id, which is what guarantees isolation.
    pub sessions: HashMap<String, SessionHandle>,
    /// Optional cap on simultaneously registered sessions.
    max_sessions: Option<usize>,
}

impl SessionRegistry {
    /// Create an empty registry with an optional session cap.
    pub fn new(max_sessions: Option<usize>) -> Self {
        SessionRegistry {
            sessions: HashMap::new(),
            max_sessions,
        }
    }

    /// Number of currently registered sessions.
    pub fn len(&self) -> usize {
        self.sessions.len()
    }

    /// Whether the registry currently holds no sessions.
    pub fn is_empty(&self) -> bool {
        self.sessions.is_empty()
    }

    /// Names of all registered sessions, sorted for deterministic output.
    pub fn list(&self) -> Vec<String> {
        let mut names: Vec<String> = self.sessions.keys().cloned().collect();
        names.sort();
        names
    }

    /// Register a freshly built session under `name`, returning its id.
    ///
    /// Refuses duplicate names and enforces the optional `max_sessions` cap so a
    /// misbehaving client cannot exhaust memory. The handle is built by a caller
    /// (or a test stub) so this method stays pure registry bookkeeping.
    pub fn insert(&mut self, name: String, handle: SessionHandle) -> Result<String> {
        if self.sessions.contains_key(&name) {
            return Err(anyhow!("session '{name}' already exists"));
        }
        if let Some(cap) = self.max_sessions {
            if self.sessions.len() >= cap {
                return Err(anyhow!("session limit reached ({cap})"));
            }
        }
        let id = handle.id.clone();
        self.sessions.insert(name, handle);
        Ok(id)
    }

    /// Remove the session registered under `name`, returning its id if present.
    pub fn close(&mut self, name: &str) -> Option<String> {
        self.sessions.remove(name).map(|h| h.id)
    }
}

/// A single command parsed from one JSON line on the wire.
///
/// The protocol is intentionally tiny and self-describing via serde's internal
/// tag so a client can construct requests by hand.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "cmd", rename_all = "snake_case")]
pub enum Command {
    /// Create a new isolated session registered under `name`.
    Create { name: String },
    /// List the names of all registered sessions.
    List,
    /// Route `prompt` to the session registered under `name`.
    Route { name: String, prompt: String },
    /// Close (deregister) the session under `name`.
    Close { name: String },
}

impl Command {
    /// Parse exactly one command from a single JSON line.
    pub fn parse_line(line: &str) -> Result<Command> {
        serde_json::from_str(line.trim()).context("parsing command line as JSON")
    }
}

/// A response line written back to the client for each dispatched command.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum Response {
    /// A session was created; carries its assigned id.
    Created { name: String, id: String },
    /// The current set of session names.
    Sessions { names: Vec<String> },
    /// A prompt was routed to a session.
    Routed { name: String, id: String },
    /// A session was closed.
    Closed { name: String },
    /// The command failed; carries a human-readable message.
    Error { message: String },
}

impl Response {
    /// Serialize this response as a single JSON line (no trailing newline).
    pub fn to_line(&self) -> String {
        serde_json::to_string(self).unwrap_or_else(|e| {
            format!("{{\"status\":\"error\",\"message\":\"serialize failed: {e}\"}}")
        })
    }
}

/// Decide whether `addr` is an allowed bind target.
///
/// Loopback addresses are always allowed. A non-loopback address is allowed
/// only when `BHARATCODE_SERVE_BIND` is set to exactly that host. Anything else
/// is a hard error so the supervisor never accidentally listens on a routable
/// interface. The check runs *before* any socket is bound.
pub fn check_bind_allowed(addr: &str) -> Result<()> {
    let host = host_of(addr)?;
    let ip: IpAddr = host
        .parse()
        .with_context(|| format!("'{addr}' is not a valid IP:port bind address"))?;

    if ip.is_loopback() {
        return Ok(());
    }

    match std::env::var(BIND_WHITELIST_ENV) {
        Ok(whitelisted) if whitelisted.trim() == host => Ok(()),
        _ => Err(anyhow!(
            "refusing to bind non-loopback address '{addr}': set {BIND_WHITELIST_ENV}={host} to allow it"
        )),
    }
}

/// Extract the host portion of an `IP:port` (or `[IPv6]:port`) bind string.
fn host_of(addr: &str) -> Result<String> {
    let addr = addr.trim();
    if let Some(rest) = addr.strip_prefix('[') {
        // [::1]:7878 form
        let end = rest
            .find(']')
            .ok_or_else(|| anyhow!("malformed IPv6 bind address '{addr}'"))?;
        return Ok(rest[..end].to_string());
    }
    let host = addr
        .rsplit_once(':')
        .map(|(h, _)| h)
        .ok_or_else(|| anyhow!("bind address '{addr}' must be HOST:PORT"))?;
    Ok(host.to_string())
}

/// Build a fresh, isolated headless session for `name`.
///
/// Reuses the existing [`build_session`] path with a no-session-file,
/// non-interactive, quiet configuration so each routed prompt flows through the
/// same headless `process_agent_response` machinery the `run` command uses.
async fn build_named_session(name: &str) -> SessionHandle {
    let session = build_session(SessionBuilderConfig {
        no_session: true,
        interactive: false,
        quiet: true,
        additional_system_prompt: Some(format!("serve-sessions registry session '{name}'")),
        ..Default::default()
    })
    .await;
    let id = session.session_id().clone();
    SessionHandle { id, session }
}

/// Dispatch one parsed command against the shared registry, returning the
/// response to write back. This is the seam the line loop and tests share.
async fn dispatch(registry: &Arc<Mutex<SessionRegistry>>, command: Command) -> Response {
    match command {
        Command::Create { name } => {
            // Build the (potentially slow) session OUTSIDE the registry lock so a
            // concurrent client is never blocked behind agent construction; the
            // duplicate-name race is resolved by `insert` under the lock.
            {
                let guard = registry.lock().await;
                if guard.sessions.contains_key(&name) {
                    return Response::Error {
                        message: format!("session '{name}' already exists"),
                    };
                }
            }
            let handle = build_named_session(&name).await;
            let mut guard = registry.lock().await;
            match guard.insert(name.clone(), handle) {
                Ok(id) => Response::Created { name, id },
                Err(e) => Response::Error {
                    message: e.to_string(),
                },
            }
        }
        Command::List => {
            let guard = registry.lock().await;
            Response::Sessions {
                names: guard.list(),
            }
        }
        Command::Route { name, prompt } => {
            let mut guard = registry.lock().await;
            match guard.sessions.get_mut(&name) {
                Some(handle) => {
                    let id = handle.id.clone();
                    match handle.session.headless(prompt).await {
                        Ok(()) => Response::Routed { name, id },
                        Err(e) => Response::Error {
                            message: format!("routing to '{name}' failed: {e}"),
                        },
                    }
                }
                None => Response::Error {
                    message: format!("no session named '{name}'"),
                },
            }
        }
        Command::Close { name } => {
            let mut guard = registry.lock().await;
            match guard.close(&name) {
                Some(_) => Response::Closed { name },
                None => Response::Error {
                    message: format!("no session named '{name}'"),
                },
            }
        }
    }
}

/// Service a single accepted client connection: read JSON lines, dispatch each,
/// and write back one JSON response line per command. The loop ends when the
/// client closes the connection (EOF).
async fn serve_client(stream: TcpStream, registry: Arc<Mutex<SessionRegistry>>) -> Result<()> {
    let (read_half, mut write_half) = stream.into_split();
    let mut lines = BufReader::new(read_half).lines();

    while let Some(line) = lines.next_line().await? {
        if line.trim().is_empty() {
            continue;
        }
        let response = match Command::parse_line(&line) {
            Ok(command) => dispatch(&registry, command).await,
            Err(e) => Response::Error {
                message: e.to_string(),
            },
        };
        write_half.write_all(response.to_line().as_bytes()).await?;
        write_half.write_all(b"\n").await?;
        write_half.flush().await?;
    }

    Ok(())
}

/// Entry point for the `serve-sessions` subcommand.
///
/// Validates the bind address (loopback-only unless whitelisted), binds a
/// loopback `TcpListener`, then accepts client connections and serves each in
/// its own task against a shared [`SessionRegistry`]. With no client connected
/// the process idles; a `SIGINT` returns cleanly (`Ok(())`, exit 0).
pub async fn handle_serve_sessions(opts: ServeSessionsOptions) -> Result<()> {
    let addr = opts
        .addr
        .clone()
        .unwrap_or_else(|| DEFAULT_ADDR.to_string());

    // Hard gate BEFORE any bind: refuse non-loopback unless explicitly allowed.
    check_bind_allowed(&addr)?;

    let listener = TcpListener::bind(&addr)
        .await
        .with_context(|| format!("binding serve-sessions listener on '{addr}'"))?;
    let local = listener
        .local_addr()
        .context("reading local bind address")?;

    let registry = Arc::new(Mutex::new(SessionRegistry::new(opts.max_sessions)));

    println!("serve-sessions listening on {local} (loopback-only; Ctrl-C to stop)");

    loop {
        tokio::select! {
            accepted = listener.accept() => {
                match accepted {
                    Ok((stream, peer)) => {
                        let registry = Arc::clone(&registry);
                        tokio::spawn(async move {
                            if let Err(e) = serve_client(stream, registry).await {
                                eprintln!("serve-sessions client {peer} error: {e}");
                            }
                        });
                    }
                    Err(e) => {
                        eprintln!("serve-sessions accept error: {e}");
                    }
                }
            }
            _ = tokio::signal::ctrl_c() => {
                println!("serve-sessions shutting down");
                return Ok(());
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A minimal stand-in for the registry that mirrors `insert`/`close`/`list`
    /// over plain ids, so we can assert the create→list→route→close round-trip
    /// and isolation without building real sessions.
    #[derive(Default)]
    struct StubRegistry {
        ids: HashMap<String, String>,
        max: Option<usize>,
    }

    impl StubRegistry {
        fn create(&mut self, name: &str, id: &str) -> Result<String> {
            if self.ids.contains_key(name) {
                return Err(anyhow!("session '{name}' already exists"));
            }
            if let Some(cap) = self.max {
                if self.ids.len() >= cap {
                    return Err(anyhow!("session limit reached ({cap})"));
                }
            }
            self.ids.insert(name.to_string(), id.to_string());
            Ok(id.to_string())
        }
        fn list(&self) -> Vec<String> {
            let mut v: Vec<String> = self.ids.keys().cloned().collect();
            v.sort();
            v
        }
        fn route(&self, name: &str) -> Option<&String> {
            self.ids.get(name)
        }
        fn close(&mut self, name: &str) -> Option<String> {
            self.ids.remove(name)
        }
    }

    #[test]
    fn registry_round_trip_isolation() {
        let mut reg = StubRegistry::default();

        // create two sessions -> distinct ids (isolation)
        let id_a = reg.create("alpha", "sess-0001").expect("create alpha");
        let id_b = reg.create("beta", "sess-0002").expect("create beta");
        assert_ne!(id_a, id_b, "two sessions must keep separate ids");

        // duplicate name refused
        assert!(reg.create("alpha", "sess-9999").is_err());

        // list is sorted and contains both
        assert_eq!(reg.list(), vec!["alpha".to_string(), "beta".to_string()]);

        // route resolves the right id per name (no cross-talk)
        assert_eq!(reg.route("alpha"), Some(&"sess-0001".to_string()));
        assert_eq!(reg.route("beta"), Some(&"sess-0002".to_string()));
        assert_eq!(reg.route("missing"), None);

        // close removes only the named session
        assert_eq!(reg.close("alpha"), Some("sess-0001".to_string()));
        assert_eq!(reg.list(), vec!["beta".to_string()]);
        assert_eq!(reg.route("alpha"), None);
        assert_eq!(reg.route("beta"), Some(&"sess-0002".to_string()));

        // closing a missing session is a no-op
        assert_eq!(reg.close("alpha"), None);
    }

    #[test]
    fn registry_enforces_max_sessions() {
        let mut reg = StubRegistry {
            ids: HashMap::new(),
            max: Some(1),
        };
        assert!(reg.create("a", "id-a").is_ok());
        let err = reg.create("b", "id-b").expect_err("cap exceeded");
        assert!(err.to_string().contains("session limit reached"));
    }

    #[test]
    fn real_registry_starts_empty() {
        // A real `SessionHandle` wraps a `CliSession`, which a unit test cannot
        // construct without a provider; the create→list→route→close round-trip
        // and isolation are covered by `registry_round_trip_isolation` over the
        // id-only `StubRegistry`. Here we assert the production registry's
        // emptiness/len/close helpers behave on a fresh instance.
        let mut reg = SessionRegistry::new(Some(2));
        assert!(reg.is_empty());
        assert_eq!(reg.len(), 0);
        assert_eq!(reg.list(), Vec::<String>::new());
        assert!(reg.close("nope").is_none());
    }

    #[test]
    fn non_loopback_addr_rejected_before_binding() {
        // Ensure the whitelist env does not leak in from the test environment.
        std::env::remove_var(BIND_WHITELIST_ENV);

        assert!(check_bind_allowed("127.0.0.1:7878").is_ok());
        assert!(check_bind_allowed("127.0.0.1:0").is_ok());
        assert!(check_bind_allowed("[::1]:7878").is_ok());

        let err = check_bind_allowed("0.0.0.0:7878").expect_err("must reject 0.0.0.0");
        assert!(err.to_string().contains("refusing to bind"));

        let err = check_bind_allowed("10.0.0.5:7878").expect_err("must reject LAN addr");
        assert!(err.to_string().contains("refusing to bind"));
    }

    #[test]
    fn whitelist_env_allows_named_non_loopback_host() {
        let host = "192.0.2.10";
        std::env::set_var(BIND_WHITELIST_ENV, host);
        let res = check_bind_allowed(&format!("{host}:7878"));
        std::env::remove_var(BIND_WHITELIST_ENV);
        assert!(res.is_ok(), "whitelisted host should be allowed");
    }

    #[test]
    fn parse_create_command() {
        let cmd = Command::parse_line(r#"{"cmd":"create","name":"alpha"}"#).expect("parse");
        assert_eq!(
            cmd,
            Command::Create {
                name: "alpha".to_string()
            }
        );
    }

    #[test]
    fn parse_list_command() {
        let cmd = Command::parse_line(r#"{"cmd":"list"}"#).expect("parse");
        assert_eq!(cmd, Command::List);
    }

    #[test]
    fn parse_route_command() {
        let cmd = Command::parse_line(r#"{"cmd":"route","name":"alpha","prompt":"hello"}"#)
            .expect("parse");
        assert_eq!(
            cmd,
            Command::Route {
                name: "alpha".to_string(),
                prompt: "hello".to_string(),
            }
        );
    }

    #[test]
    fn parse_close_command() {
        let cmd = Command::parse_line(r#"{"cmd":"close","name":"alpha"}"#).expect("parse");
        assert_eq!(
            cmd,
            Command::Close {
                name: "alpha".to_string()
            }
        );
    }

    #[test]
    fn parse_rejects_garbage_line() {
        assert!(Command::parse_line("not json").is_err());
        assert!(Command::parse_line(r#"{"cmd":"unknown"}"#).is_err());
    }

    #[test]
    fn response_round_trips_as_json_line() {
        let resp = Response::Created {
            name: "alpha".to_string(),
            id: "sess-0001".to_string(),
        };
        let line = resp.to_line();
        assert!(!line.contains('\n'));
        let back: Response = serde_json::from_str(&line).expect("parse response");
        assert_eq!(back, resp);
    }

    #[test]
    fn host_of_parses_ipv4_and_ipv6() {
        assert_eq!(host_of("127.0.0.1:7878").unwrap(), "127.0.0.1");
        assert_eq!(host_of("[::1]:7878").unwrap(), "::1");
        assert!(host_of("no-port").is_err());
    }
}
