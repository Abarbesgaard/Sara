use std::future::Future;

use rmcp::RoleServer;
use rmcp::model::{
    ClientJsonRpcMessage, ClientRequest, ErrorCode, ErrorData, RequestId, ServerJsonRpcMessage,
};
use rmcp::transport::Transport;

pub(crate) struct TolerantInit<T> {
    inner: T,
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

enum PreInit {
    Forward,
    Reject(ErrorData, RequestId),
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
#[path = "../../tests/unit/mcp/transport.rs"]
mod tests;
