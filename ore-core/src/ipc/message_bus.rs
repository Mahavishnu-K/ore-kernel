use dashmap::DashMap;
use serde::{Deserialize, Serialize};
use std::sync::Mutex as StdMutex;
use tokio::sync::mpsc;

pub const DEFAULT_MESSAGE_BACKLOG: usize = 1024;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AgentMessage {
    pub from_app: String,
    pub to_app: String,
    pub payload: String,
    pub timestamp: u64,
}

pub struct MessageBus {
    channel: DashMap<
        String,
        (
            mpsc::Sender<AgentMessage>,
            StdMutex<mpsc::Receiver<AgentMessage>>,
        ),
    >,
    backlog_capacity: usize,
}

impl Default for MessageBus {
    fn default() -> Self {
        Self::new()
    }
}

impl MessageBus {
    pub fn new() -> Self {
        Self {
            channel: DashMap::new(),
            backlog_capacity: DEFAULT_MESSAGE_BACKLOG,
        }
    }

    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            channel: DashMap::new(),
            backlog_capacity: capacity.max(1),
        }
    }

    /// App A sends a message to App B with bounded queue protection (Prevents OOM)
    pub fn send_message(&self, msg: AgentMessage) -> Result<(), String> {
        let target_channel = self.channel.entry(msg.to_app.clone()).or_insert_with(|| {
            let (tx, rx) = mpsc::channel(self.backlog_capacity);
            (tx, StdMutex::new(rx))
        });

        target_channel.0.try_send(msg).map_err(|e| match e {
            mpsc::error::TrySendError::Full(dropped) => {
                format!(
                    "Target agent '{}' queue is full (capacity: {}). Backpressure applied.",
                    dropped.to_app, self.backlog_capacity
                )
            }
            mpsc::error::TrySendError::Closed(_) => {
                "Target agent channel receiver has closed.".to_string()
            }
        })
    }

    /// App B reads next pending message non-blockingly
    pub fn read_message(&self, app_id: &str) -> Option<AgentMessage> {
        if let Some(target_channel) = self.channel.get(app_id) {
            let mut rx = target_channel.1.lock().unwrap();
            if let Ok(msg) = rx.try_recv() {
                return Some(msg);
            }
        }
        None
    }
}
