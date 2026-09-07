//! Pre-initialize tolerance for the MCP stdio transport.
//!
//! rmcp's server handshake accepts only `initialize` (and `ping`) as the first
//! client message and treats anything else as fatal, so the whole server exits
//! with code 1 before it ever comes up. GitHub Copilot CLI probes with a custom
//! `server/discover` request *before* sending `initialize`, which killed every
//! `sara mcp` session under it.
//!
//! [`TolerantInit`] wraps the underlying transport and absorbs that pre-init
//! traffic instead: unknown requests are answered with a JSON-RPC
//! `method not found` error (exactly what rmcp itself replies after the
//! handshake), stray pre-init notifications/responses are dropped, and from the
//! first `initialize` onward every message passes through untouched.

use std::future::Future;

use rmcp::RoleServer;
use rmcp::model::{
    ClientJsonRpcMessage, ClientRequest, ErrorCode, ErrorData, RequestId, ServerJsonRpcMessage,
};
use rmcp::transport::Transport;

/// Wraps a server-side [`Transport`] so spec-violating client traffic before
/// `initialize` cannot abort rmcp's handshake.
pub(crate) struct TolerantInit<T> {
    inner: T,
    /// Set once the client's `initialize` request has been forwarded; from then
    /// on the wrapper is a pure passthrough (rmcp handles unknown methods
    /// itself post-handshake).
    initialize_seen: bool,
}

impl<T> TolerantInit<T> {
    pub(crate) fn new(inner: T) -> Self {
        Self {
            inner,
            initialize_seen: false,
        }
    }
}

/// What to do with a message received before `initialize`.
enum PreInit {
    /// Hand the message to rmcp (it is `initialize`, or a `ping` rmcp answers
    /// itself during the handshake).
    Forward,
    /// Answer with a `method not found` error and keep waiting.
    Reject(ErrorData, RequestId),
    /// Silently discard (pre-init notifications/responses, which rmcp would
    /// treat as fatal).
    Drop,
}

impl<T: Transport<RoleServer>> Transport<RoleServer> for TolerantInit<T> {
    type Error = T::Error;

    fn send(
        &mut self,
        item: ServerJsonRpcMessage,
    ) -> impl Future<Output = Result<(), Self::Error>> + Send + 'static {
        self.inner.send(item)
    }

    async fn receive(&mut self) -> Option<ClientJsonRpcMessage> {
        loop {
            let msg = self.inner.receive().await?;
            if self.initialize_seen {
                return Some(msg);
            }
            let decision = match &msg {
                ClientJsonRpcMessage::Request(req) => match &req.request {
                    ClientRequest::InitializeRequest(_) => {
                        self.initialize_seen = true;
                        PreInit::Forward
                    }
                    ClientRequest::PingRequest(_) => PreInit::Forward,
                    // E.g. Copilot CLI's `server/discover` probe.
                    other => PreInit::Reject(
                        ErrorData::new(
                            ErrorCode::METHOD_NOT_FOUND,
                            other.method().to_string(),
                            None,
                        ),
                        req.id.clone(),
                    ),
                },
                _ => PreInit::Drop,
            };
            match decision {
                PreInit::Forward => return Some(msg),
                PreInit::Reject(err, id) => {
                    // Best effort: if the reply fails the client will time the
                    // probe out; the handshake itself must stay alive.
                    let _ = self
                        .inner
                        .send(ServerJsonRpcMessage::error(err, Some(id)))
                        .await;
                }
                PreInit::Drop => {}
            }
        }
    }

    fn close(&mut self) -> impl Future<Output = Result<(), Self::Error>> + Send {
        self.inner.close()
    }
}

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;
    use std::sync::{Arc, Mutex};

    use serde_json::json;

    use super::*;

    /// In-memory transport: pops scripted incoming messages, records sends.
    struct Mock {
        incoming: VecDeque<ClientJsonRpcMessage>,
        sent: Arc<Mutex<Vec<ServerJsonRpcMessage>>>,
    }

    impl Mock {
        fn new(msgs: Vec<serde_json::Value>) -> (Self, Arc<Mutex<Vec<ServerJsonRpcMessage>>>) {
            let sent = Arc::new(Mutex::new(Vec::new()));
            let incoming = msgs
                .into_iter()
                .map(|v| serde_json::from_value(v).expect("valid client message"))
                .collect();
            (
                Self {
                    incoming,
                    sent: sent.clone(),
                },
                sent,
            )
        }
    }

    impl Transport<RoleServer> for Mock {
        type Error = std::io::Error;

        fn send(
            &mut self,
            item: ServerJsonRpcMessage,
        ) -> impl Future<Output = Result<(), Self::Error>> + Send + 'static {
            let sent = self.sent.clone();
            async move {
                sent.lock().unwrap().push(item);
                Ok(())
            }
        }

        async fn receive(&mut self) -> Option<ClientJsonRpcMessage> {
            self.incoming.pop_front()
        }

        async fn close(&mut self) -> Result<(), Self::Error> {
            Ok(())
        }
    }

    fn discover(id: u32) -> serde_json::Value {
        json!({"jsonrpc": "2.0", "id": id, "method": "server/discover", "params": {}})
    }

    fn initialize(id: u32) -> serde_json::Value {
        json!({"jsonrpc": "2.0", "id": id, "method": "initialize", "params": {
            "protocolVersion": "2025-06-18",
            "capabilities": {},
            "clientInfo": {"name": "test", "version": "0"}
        }})
    }

    fn method_of(msg: &ClientJsonRpcMessage) -> Option<&str> {
        match msg {
            ClientJsonRpcMessage::Request(r) => Some(r.request.method()),
            _ => None,
        }
    }

    #[tokio::test]
    async fn pre_init_discover_is_rejected_and_initialize_forwarded() {
        // Copilot CLI's exact opening: server/discover BEFORE initialize.
        let (mock, sent) = Mock::new(vec![discover(0), initialize(1)]);
        let mut t = TolerantInit::new(mock);

        // The first message rmcp sees must be initialize — the probe is absorbed.
        let first = t.receive().await.expect("message");
        assert_eq!(method_of(&first), Some("initialize"));

        // The probe got a method-not-found error addressed to its id.
        let sent = sent.lock().unwrap();
        assert_eq!(sent.len(), 1, "exactly one pre-init reply");
        let v = serde_json::to_value(&sent[0]).unwrap();
        assert_eq!(v["id"], 0);
        assert_eq!(v["error"]["code"], -32601);
        assert_eq!(v["error"]["message"], "server/discover");
    }

    #[tokio::test]
    async fn post_init_messages_pass_through_untouched() {
        // After initialize, unknown requests are rmcp's business (-32601 from
        // its own dispatch), not the wrapper's.
        let (mock, sent) = Mock::new(vec![initialize(0), discover(1)]);
        let mut t = TolerantInit::new(mock);

        assert_eq!(method_of(&t.receive().await.unwrap()), Some("initialize"));
        assert_eq!(
            method_of(&t.receive().await.unwrap()),
            Some("server/discover")
        );
        assert!(sent.lock().unwrap().is_empty(), "wrapper sent nothing");
    }

    #[tokio::test]
    async fn pre_init_notifications_are_dropped() {
        let notif = json!({"jsonrpc": "2.0", "method": "notifications/cancelled", "params": {"requestId": 9}});
        let (mock, sent) = Mock::new(vec![notif, initialize(0)]);
        let mut t = TolerantInit::new(mock);

        assert_eq!(method_of(&t.receive().await.unwrap()), Some("initialize"));
        assert!(sent.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn pre_init_ping_is_forwarded_for_rmcp_to_answer() {
        let ping = json!({"jsonrpc": "2.0", "id": 0, "method": "ping"});
        let (mock, sent) = Mock::new(vec![ping, initialize(1)]);
        let mut t = TolerantInit::new(mock);

        // rmcp's handshake loop answers pre-init pings itself; forward them.
        assert_eq!(method_of(&t.receive().await.unwrap()), Some("ping"));
        assert_eq!(method_of(&t.receive().await.unwrap()), Some("initialize"));
        assert!(sent.lock().unwrap().is_empty());
    }
}
