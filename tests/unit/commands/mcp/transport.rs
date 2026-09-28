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
    let notif =
        json!({"jsonrpc": "2.0", "method": "notifications/cancelled", "params": {"requestId": 9}});
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
