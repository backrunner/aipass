//! Native background work outlives an HTTP response, but still owns its session.
use super::*;
use std::collections::HashSet;

#[derive(Default)]
pub(super) struct Lifecycle {
    tasks: HashSet<String>,
    awaiting: HashSet<String>,
    pub(super) autonomous: bool,
    pub(super) pending_output: bool,
    pub(super) foreground_sent: bool,
    pub(super) queued_user: Option<Value>,
}
impl Lifecycle {
    pub(super) fn observe(&mut self, value: &Value) -> Result<(), String> {
        if value["type"] == "system" {
            match value["subtype"].as_str() {
                Some("background_tasks_changed") => {
                    self.tasks = value["tasks"]
                        .as_array()
                        .into_iter()
                        .flatten()
                        .filter_map(|v| v["task_id"].as_str())
                        .map(str::to_owned)
                        .collect();
                    self.awaiting.extend(self.tasks.iter().cloned());
                }
                // task_started also describes foreground subagents. Only the
                // native background roster establishes durable background work.
                Some("task_notification") => {
                    if let Some(id) = value["task_id"].as_str() {
                        self.tasks.remove(id);
                        self.awaiting.remove(id);
                    }
                    self.pending_output |= !self.foreground_sent;
                }
                _ => {}
            }
        }
        if self.tasks.len().max(self.awaiting.len()) > 4096 {
            return Err("too many Claude background tasks".into());
        }
        if !self.foreground_sent {
            let main_output = value["parent_tool_use_id"].is_null()
                && (value["type"] == "system" && value["subtype"] == "init"
                    || matches!(
                        value["type"].as_str(),
                        Some("assistant" | "stream_event" | "result")
                    ));
            self.autonomous |= main_output;
            self.pending_output |= main_output
                || matches!(
                    value["type"].as_str(),
                    Some("assistant" | "user" | "control_request")
                );
        }
        Ok(())
    }
    pub(super) fn active(&self) -> bool {
        !self.tasks.is_empty() || !self.awaiting.is_empty() || self.autonomous
    }
    pub(super) fn pending(&self) -> bool {
        self.active() || self.pending_output
    }
    pub(super) fn send(&mut self, stdin: &mut ChildStdin, user: Value) -> Result<(), String> {
        if self.autonomous {
            self.queued_user = Some(user);
        } else {
            write(stdin, &user)?;
            self.foreground_sent = true;
        }
        Ok(())
    }
    pub(super) fn settled(&mut self, stdin: &mut ChildStdin) -> Result<bool, String> {
        let autonomous = self.autonomous || !self.foreground_sent;
        if autonomous {
            self.autonomous = false;
            if let Some(user) = self.queued_user.take() {
                self.send(stdin, user)?;
            }
        } else {
            self.foreground_sent = false;
            self.pending_output = false;
        }
        Ok(autonomous)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn empty_roster_waits_for_notifications_but_foreground_tasks_do_not_pin() {
        let mut state = Lifecycle::default();
        state
            .observe(&json!({"type":"system","subtype":"task_started","task_id":"foreground"}))
            .unwrap();
        assert!(!state.active());
        state.observe(&json!({"type":"system","subtype":"background_tasks_changed","tasks":[{"task_id":"bg"}]})).unwrap();
        assert!(state.active());
        state
            .observe(&json!({"type":"system","subtype":"background_tasks_changed","tasks":[]}))
            .unwrap();
        assert!(state.active());
        state.observe(&json!({"type":"system","subtype":"task_notification","task_id":"bg","status":"failed"})).unwrap();
        assert!(!state.active());
        assert!(state.pending());
    }
}
