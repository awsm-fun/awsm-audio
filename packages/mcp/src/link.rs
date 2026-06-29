//! The link between MCP agents and the attached editor tab.
//!
//! This server is **single-session by design**: one MCP server serves exactly
//! one editor tab and one agent. There is no pairing, no disambiguation, no
//! multi-tab routing — if you want a second concurrent session, run a second
//! server on another port and point a second editor at it.
//!
//! Two identities still meet here:
//!   - a [`Connection`] is one editor tab (one `/editor` WebSocket), with its own
//!     request-id space, pending-request map, and writer.
//!   - an [`AgentSession`] is one MCP client (one `EditorMcp`).
//!
//! An agent's requests go to the most-recently-attached tab; an event from that
//! tab is delivered to the agent. When a new tab attaches, any previously
//! attached tab is **evicted** (told it's [`detached`](WsServerMsg::Detached) and
//! dropped) so exactly one live tab remains. This makes binding deterministic and
//! self-healing across MCP/server restarts: each reconnect simply becomes *the*
//! session.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use tokio::sync::{broadcast, mpsc, oneshot};

use awsm_audio_editor_protocol::{EditorEvent, Request, Response, WsServerMsg};

/// Upper bound on one request's round-trip (offline renders are the slow case).
const REQUEST_TIMEOUT: Duration = Duration::from_secs(120);

/// Why a request couldn't be delivered.
pub enum LinkError {
    /// No editor tab is attached.
    Transport(String),
}

/// One attached editor tab.
pub struct Connection {
    pub id: u64,
    tx: mpsc::UnboundedSender<WsServerMsg>,
    pending: Mutex<HashMap<u64, oneshot::Sender<Response>>>,
    next_req_id: AtomicU64,
}

impl Connection {
    /// Send one request to this tab and await its response.
    async fn request(&self, req: &Request) -> Result<Response, String> {
        let id = self.next_req_id.fetch_add(1, Ordering::Relaxed);
        let (tx, rx) = oneshot::channel();
        self.pending.lock().unwrap().insert(id, tx);
        self.tx
            .send(WsServerMsg::Request {
                id,
                req: req.clone(),
            })
            .map_err(|_| "editor link closed".to_string())?;
        match tokio::time::timeout(REQUEST_TIMEOUT, rx).await {
            Ok(Ok(resp)) => Ok(resp),
            Ok(Err(_)) => {
                self.pending.lock().unwrap().remove(&id);
                Err("editor dropped the request".into())
            }
            Err(_) => {
                self.pending.lock().unwrap().remove(&id);
                Err("editor request timed out".into())
            }
        }
    }

    /// Complete a pending request from an incoming `Response` frame.
    pub fn complete(&self, id: u64, resp: Response) {
        if let Some(tx) = self.pending.lock().unwrap().remove(&id) {
            let _ = tx.send(resp);
        }
    }

    /// Push a server→browser frame (best-effort).
    pub fn send(&self, msg: WsServerMsg) {
        let _ = self.tx.send(msg);
    }

    /// Fail every in-flight request (on socket close): dropping the senders makes
    /// each awaiting `request` resolve to the "dropped" error.
    fn drain(&self) {
        self.pending.lock().unwrap().clear();
    }
}

/// One connected MCP agent (one `EditorMcp`, one `Mcp-Session-Id`). Kept as a
/// lightweight identity handle; in the single-session model it carries no
/// binding state (every agent talks to the one attached tab).
pub struct AgentSession {
    pub id: u64,
}

struct LinkInner {
    connections: Mutex<Vec<Arc<Connection>>>,
    /// Editor push events, tagged with the originating connection id (the agent
    /// forwarder keeps only the live tab's events).
    events: broadcast::Sender<(u64, EditorEvent)>,
    /// Agent-supplied audio files hosted for the editor to fetch (id → bytes +
    /// content-type), so a `load_audio` from a local path never rides the link.
    assets: Mutex<HashMap<String, (Vec<u8>, String)>>,
    /// This server's own origin (e.g. `http://127.0.0.1:9171`), used to build
    /// `/assets/<id>` URLs the editor fetches.
    self_origin: String,
    next_conn_id: AtomicU64,
    next_agent_id: AtomicU64,
}

/// Shared handle to the agent/connection registry. Cheap to clone (`Arc`).
#[derive(Clone)]
pub struct EditorLink {
    inner: Arc<LinkInner>,
}

impl EditorLink {
    pub fn shared(self_origin: String) -> Self {
        let (events, _) = broadcast::channel(256);
        Self {
            inner: Arc::new(LinkInner {
                connections: Mutex::new(Vec::new()),
                events,
                assets: Mutex::new(HashMap::new()),
                self_origin,
                next_conn_id: AtomicU64::new(1),
                next_agent_id: AtomicU64::new(1),
            }),
        }
    }

    /// This server's origin (for building `/assets/<id>` and `/renders/<id>` URLs).
    pub fn self_origin(&self) -> &str {
        &self.inner.self_origin
    }

    /// Host an agent-supplied audio file for the editor to fetch.
    pub fn store_asset(&self, id: String, bytes: Vec<u8>, content_type: String) {
        self.inner
            .assets
            .lock()
            .unwrap()
            .insert(id, (bytes, content_type));
    }

    /// Fetch a hosted audio file (for the `/assets/<id>` route).
    pub fn asset_bytes(&self, id: &str) -> Option<(Vec<u8>, String)> {
        self.inner.assets.lock().unwrap().get(id).cloned()
    }

    // ── connections (editor tabs) ───────────────────────────────────────────

    /// Register a freshly-attached tab, returning its [`Connection`]. Enforces the
    /// single-session model: any previously-attached tab is evicted (told it's
    /// [`Detached`](WsServerMsg::Detached), its in-flight requests failed, and it
    /// is dropped) so this becomes the one live tab.
    pub fn register_connection(&self, tx: mpsc::UnboundedSender<WsServerMsg>) -> Arc<Connection> {
        let id = self.inner.next_conn_id.fetch_add(1, Ordering::Relaxed);
        let conn = Arc::new(Connection {
            id,
            tx,
            pending: Mutex::new(HashMap::new()),
            next_req_id: AtomicU64::new(1),
        });
        let mut conns = self.inner.connections.lock().unwrap();
        // Evict every prior tab — one server, one tab.
        for old in conns.drain(..) {
            tracing::info!(
                "editor connection {} superseded by {} — evicting",
                old.id,
                id
            );
            old.send(WsServerMsg::Detached);
            old.drain();
        }
        conns.push(conn.clone());
        conn
    }

    /// Remove a tab on socket close: fail its in-flight requests and forget it.
    pub fn remove_connection(&self, id: u64) {
        let mut conns = self.inner.connections.lock().unwrap();
        if let Some(pos) = conns.iter().position(|c| c.id == id) {
            let conn = conns.remove(pos);
            conn.drain();
        }
    }

    /// How many editor tabs are currently connected (0 or 1 in normal operation).
    pub fn connection_count(&self) -> usize {
        self.inner.connections.lock().unwrap().len()
    }

    /// Publish an editor push event (tagged with its originating connection).
    pub fn publish_event(&self, conn_id: u64, ev: EditorEvent) {
        let _ = self.inner.events.send((conn_id, ev));
    }

    pub fn subscribe_events(&self) -> broadcast::Receiver<(u64, EditorEvent)> {
        self.inner.events.subscribe()
    }

    // ── agents (MCP sessions) ───────────────────────────────────────────────

    /// Register a new agent session (called once per `EditorMcp`).
    pub fn register_agent(&self) -> Arc<AgentSession> {
        let id = self.inner.next_agent_id.fetch_add(1, Ordering::Relaxed);
        let agent = Arc::new(AgentSession { id });
        tracing::debug!("agent session {} registered", agent.id);
        agent
    }

    /// The tab an agent talks to: the single attached tab (the most recent, if a
    /// stale one lingers), else [`LinkError::Transport`].
    pub fn resolve(&self, _agent: &Arc<AgentSession>) -> Result<Arc<Connection>, LinkError> {
        self.inner
            .connections
            .lock()
            .unwrap()
            .last()
            .cloned()
            .ok_or_else(|| {
                LinkError::Transport(
                    "no editor tab is attached to this MCP server. Open the awsm-audio \
                     editor pointed at this server (append `?mcp=<this server's origin>` to \
                     its URL, or use its MCP connect modal) and wait for it to connect."
                        .into(),
                )
            })
    }

    /// The id of the tab requests currently route to (for the event forwarder).
    pub fn current_conn_id(&self) -> Option<u64> {
        self.inner.connections.lock().unwrap().last().map(|c| c.id)
    }

    /// Send a request from `agent` to the attached tab.
    pub async fn request(
        &self,
        agent: &Arc<AgentSession>,
        req: &Request,
    ) -> Result<Response, LinkError> {
        let conn = self.resolve(agent)?;
        conn.request(req).await.map_err(LinkError::Transport)
    }

    /// Best-effort request for the dev `/debug` seam (no agent): use the only /
    /// most-recently-attached tab.
    pub async fn debug_request(&self, req: &Request) -> Result<Response, String> {
        let conn = self
            .inner
            .connections
            .lock()
            .unwrap()
            .last()
            .cloned()
            .ok_or_else(|| "no editor attached".to_string())?;
        conn.request(req).await
    }
}
