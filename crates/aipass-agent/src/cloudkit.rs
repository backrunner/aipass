//! CloudKit runs in the signed macOS shell. The agent owns all vault decisions;
//! the authenticated bridge receives only immutable ciphertext transport work.
use aipass_agent_protocol::{CloudKitCommand, CloudKitCompletion, CloudKitReply, CloudKitTask};
use aipass_sync::{snapshot_id, valid_snapshot_id, SnapshotRemote};
use anyhow::{bail, Context, Result};
use base64::{engine::general_purpose::STANDARD, Engine};
use std::collections::VecDeque;
use std::sync::{mpsc, Condvar, Mutex};
use std::time::{Duration, Instant};

#[derive(Debug)]
pub(crate) struct TransportError {
    pub kind: aipass_agent_protocol::CloudKitErrorKind,
    message: String,
}
impl std::fmt::Display for TransportError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}
impl std::error::Error for TransportError {}

#[derive(Default)]
struct Queue {
    waiting: VecDeque<(CloudKitTask, mpsc::Sender<CloudKitReply>)>,
    active: Option<(CloudKitTask, mpsc::Sender<CloudKitReply>)>,
    last_poll: Option<Instant>,
}

#[derive(Default)]
pub struct CloudKitBridge {
    queue: Mutex<Queue>,
    wake: Condvar,
}

impl CloudKitBridge {
    pub fn exchange(&self, completion: Option<CloudKitCompletion>) -> Result<Option<CloudKitTask>> {
        let mut queue = self
            .queue
            .lock()
            .map_err(|_| anyhow::anyhow!("CloudKit queue poisoned"))?;
        queue.last_poll = Some(Instant::now());
        self.wake.notify_all();
        if let Some(completion) = completion {
            if queue
                .active
                .as_ref()
                .is_some_and(|(task, _)| task.id == completion.id)
            {
                let (_, sender) = queue.active.take().unwrap();
                let _ = sender.send(completion.reply);
            }
        }
        if let Some((task, _)) = &queue.active {
            // Replay an unacknowledged delivery after IPC loss or shell restart.
            // Snapshot writes are immutable/idempotent; late completions cannot
            // cancel another operation or remove its waiter.
            return Ok(Some(task.clone()));
        }
        if queue.waiting.is_empty() {
            queue = self
                .wake
                .wait_timeout(queue, Duration::from_millis(500))
                .map_err(|_| anyhow::anyhow!("CloudKit queue poisoned"))?
                .0;
        }
        if let Some((task, sender)) = queue.waiting.pop_front() {
            queue.active = Some((task.clone(), sender));
            return Ok(Some(task));
        }
        Ok(None)
    }

    fn request(&self, command: CloudKitCommand, account: Option<String>) -> Result<CloudKitReply> {
        if !cfg!(any(target_os = "macos", test)) {
            bail!("CloudKit requires the macOS desktop app");
        }
        let (sender, receiver) = mpsc::channel();
        let expected_account = account.clone();
        let id = uuid::Uuid::new_v4();
        let mut queue = self
            .queue
            .lock()
            .map_err(|_| anyhow::anyhow!("CloudKit queue poisoned"))?;
        if queue
            .last_poll
            .is_none_or(|at| at.elapsed() > Duration::from_secs(60))
        {
            queue = self
                .wake
                .wait_timeout_while(queue, Duration::from_secs(2), |queue| {
                    queue
                        .last_poll
                        .is_none_or(|at| at.elapsed() > Duration::from_secs(60))
                })
                .map_err(|_| anyhow::anyhow!("CloudKit queue poisoned"))?
                .0;
            if queue
                .last_poll
                .is_none_or(|at| at.elapsed() > Duration::from_secs(60))
            {
                bail!("CloudKit transport unavailable; open the signed macOS app");
            }
        }
        queue.waiting.push_back((
            CloudKitTask {
                id,
                command,
                account,
            },
            sender,
        ));
        drop(queue);
        self.wake.notify_one();
        let result = receiver.recv_timeout(Duration::from_secs(45));
        let mut queue = self
            .queue
            .lock()
            .map_err(|_| anyhow::anyhow!("CloudKit queue poisoned"))?;
        queue.waiting.retain(|(task, _)| task.id != id);
        if queue
            .active
            .as_ref()
            .is_some_and(|(active, _)| active.id == id)
        {
            queue.active.take();
        }
        let reply = result.context("CloudKit transport unavailable; open the signed macOS app")?;
        if let Some(error) = &reply.error {
            return Err(TransportError {
                kind: reply
                    .error_kind
                    .unwrap_or(aipass_agent_protocol::CloudKitErrorKind::Unavailable),
                message: format!("CloudKit: {error}"),
            }
            .into());
        }
        if expected_account.is_some() && reply.account != expected_account {
            return Err(TransportError {
                kind: aipass_agent_protocol::CloudKitErrorKind::AccountChanged,
                message: "CloudKit account changed during synchronization".into(),
            }
            .into());
        }
        Ok(reply)
    }
}

pub(crate) struct Remote<'a> {
    bridge: &'a CloudKitBridge,
    pub account: String,
    ids: Vec<String>,
}

impl<'a> Remote<'a> {
    pub fn connect(bridge: &'a CloudKitBridge) -> Result<Self> {
        let reply = bridge.request(CloudKitCommand::List, None)?;
        let account = reply.account.context("CloudKit account identity missing")?;
        if !valid_snapshot_id(&account) || reply.ids.iter().any(|id| !valid_snapshot_id(id)) {
            bail!("invalid CloudKit response");
        }
        Ok(Self {
            bridge,
            account,
            ids: reply.ids,
        })
    }
}

impl SnapshotRemote for Remote<'_> {
    fn list(&self) -> Result<Vec<String>> {
        Ok(self.ids.clone())
    }
    fn get(&self, id: &str) -> Result<Vec<u8>> {
        if !valid_snapshot_id(id) {
            bail!("invalid snapshot id");
        }
        let reply = self.bridge.request(
            CloudKitCommand::Get { id: id.into() },
            Some(self.account.clone()),
        )?;
        let bytes = STANDARD.decode(reply.bytes_b64.context("CloudKit snapshot missing")?)?;
        if snapshot_id(&bytes) != id {
            bail!("CloudKit snapshot hash mismatch");
        }
        Ok(bytes)
    }
    fn publish(&self, bytes: &[u8]) -> Result<String> {
        let id = snapshot_id(bytes);
        // Framed IPC has a 16 MiB bound. Refuse before enqueuing instead of
        // leaving a transport task that can never be delivered.
        if bytes.len() > 11 * 1024 * 1024 {
            bail!("CloudKit snapshot exceeds the 11 MiB transport limit");
        }
        self.bridge.request(
            CloudKitCommand::Put {
                id: id.clone(),
                bytes_b64: STANDARD.encode(bytes),
            },
            Some(self.account.clone()),
        )?;
        Ok(id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Instant;
    #[test]
    fn bridge_rejects_missing_or_changed_account_acknowledgements() {
        for account in [None, Some("b".repeat(64))] {
            let bridge = std::sync::Arc::new(CloudKitBridge::default());
            let worker = bridge.clone();
            let thread = std::thread::spawn(move || {
                worker.request(
                    CloudKitCommand::Put {
                        id: "c".repeat(64),
                        bytes_b64: String::new(),
                    },
                    Some("a".repeat(64)),
                )
            });
            let task = loop {
                if let Some(task) = bridge.exchange(None).unwrap() {
                    break task;
                }
            };
            bridge
                .exchange(Some(CloudKitCompletion {
                    id: task.id,
                    reply: CloudKitReply {
                        account,
                        ..Default::default()
                    },
                }))
                .unwrap();
            let error = thread.join().unwrap().unwrap_err();
            assert!(matches!(
                error.downcast_ref::<TransportError>().unwrap().kind,
                aipass_agent_protocol::CloudKitErrorKind::AccountChanged
            ));
        }
    }
    #[test]
    fn bridge_delivers_only_the_matching_reply() {
        let bridge = std::sync::Arc::new(CloudKitBridge::default());
        let worker = bridge.clone();
        let thread = std::thread::spawn(move || worker.request(CloudKitCommand::List, None));
        let start = Instant::now();
        let task = loop {
            if let Some(task) = bridge.exchange(None).unwrap() {
                break task;
            }
            assert!(start.elapsed() < Duration::from_secs(3));
        };
        let redelivered = bridge
            .exchange(Some(CloudKitCompletion {
                id: uuid::Uuid::new_v4(),
                reply: CloudKitReply {
                    error: Some("late reply from another operation".into()),
                    ..Default::default()
                },
            }))
            .unwrap()
            .unwrap();
        assert_eq!(redelivered.id, task.id);
        bridge
            .exchange(Some(CloudKitCompletion {
                id: task.id,
                reply: CloudKitReply {
                    account: Some("a".repeat(64)),
                    ..Default::default()
                },
            }))
            .unwrap();
        assert_eq!(
            thread.join().unwrap().unwrap().account,
            Some("a".repeat(64))
        );
    }
}
