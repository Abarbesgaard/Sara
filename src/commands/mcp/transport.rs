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
#[path = "../../../tests/unit/commands/mcp/transport.rs"]
mod tests;
