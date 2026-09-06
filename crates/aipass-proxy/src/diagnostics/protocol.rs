//! Allowlisted protocol metadata only. Never format wire values into logs.
use super::*;
use serde::de::{IgnoredAny, SeqAccess, Visitor};
use serde_json::Value;
use std::{collections::HashSet, fmt};

#[derive(Default, Deserialize)]
#[serde(rename_all = "snake_case")]
enum ToolKind {
    Function,
    Custom,
    Shell,
    LocalShell,
    Namespace,
    #[default]
    #[serde(other)]
    Other,
}

#[derive(Default, Deserialize)]
#[serde(rename_all = "snake_case")]
enum ToolName {
    Exec,
    ExecCommand,
    Shell,
    #[default]
    #[serde(other)]
    Other,
}

#[derive(Default, Deserialize)]
struct Tool {
    #[serde(default, rename = "type")]
    kind: ToolKind,
    #[serde(default)]
    name: ToolName,
    #[serde(default)]
    tools: ToolCounts,
}

#[derive(Default)]
struct ToolCounts {
    total: u64,
    function: u64,
    custom: u64,
    shell: u64,
    namespace: u64,
    other: u64,
    exec: u64,
}

impl<'de> Deserialize<'de> for ToolCounts {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct Tools;
        impl<'de> Visitor<'de> for Tools {
            type Value = ToolCounts;
            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("a tool array")
            }
            fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<ToolCounts, A::Error> {
                let mut counts = ToolCounts::default();
                while let Some(tool) = seq.next_element::<Tool>()? {
                    counts.total += 1 + tool.tools.total;
                    counts.function +=
                        tool.tools.function + u64::from(matches!(tool.kind, ToolKind::Function));
                    counts.custom +=
                        tool.tools.custom + u64::from(matches!(tool.kind, ToolKind::Custom));
                    counts.shell += tool.tools.shell
                        + u64::from(matches!(tool.kind, ToolKind::Shell | ToolKind::LocalShell));
                    counts.namespace +=
                        tool.tools.namespace + u64::from(matches!(tool.kind, ToolKind::Namespace));
                    counts.other +=
                        tool.tools.other + u64::from(matches!(tool.kind, ToolKind::Other));
                    counts.exec +=
                        tool.tools.exec + u64::from(!matches!(tool.name, ToolName::Other));
                }
                Ok(counts)
            }
        }
        deserializer.deserialize_seq(Tools)
    }
}

// Unknown fields (including input, descriptions and schemas) are skipped by
// serde, so file-backed requests need not be materialized to inspect tools.
#[derive(Default, Deserialize)]
pub(crate) struct RequestSummary {
    tools: Option<ToolCounts>,
    previous_response_id: Option<IgnoredAny>,
}

impl RequestSummary {
    pub(crate) fn log(
        summary: Option<&Self>,
        store: &UsageStore,
        request_id: Uuid,
        stage: &'static str,
    ) {
        let Some(summary) = summary else {
            store.log_diagnostic("info", format!("event=proxy.tools.summary request_id={request_id} stage={stage} available=false"));
            return;
        };
        let empty = ToolCounts::default();
        let counts = summary.tools.as_ref().unwrap_or(&empty);
        store.log_diagnostic("info", format!(
            "event=proxy.tools.summary request_id={request_id} stage={stage} available=true tools_present={} tools={} function={} custom={} shell={} namespace={} other={} exec={} previous_response={}",
            summary.tools.is_some(), counts.total, counts.function, counts.custom, counts.shell,
            counts.namespace, counts.other, counts.exec, summary.previous_response_id.is_some(),
        ));
    }
}

/// Bounded, request-local counters. Output indices are never written to disk.
#[derive(Default)]
pub(crate) struct ResponseTrace {
    events: u64,
    created: u64,
    items: u64,
    tool_items: u64,
    text_deltas: u64,
    tool_deltas: u64,
    terminal: u64,
    delta_without_item: u64,
    sequence_regressions: u64,
    unindexed_deltas: u64,
    tracking_limited: bool,
    last_sequence: Option<u64>,
    active_items: HashSet<u64>,
    last_event: &'static str,
}

impl ResponseTrace {
    pub(crate) fn observe(&mut self, value: &Value) {
        self.events += 1;
        if let Some(sequence) = value["sequence_number"].as_u64() {
            if self.last_sequence.is_some_and(|last| sequence <= last) {
                self.sequence_regressions += 1;
            }
            self.last_sequence = Some(sequence);
        }
        let kind = value["type"].as_str().unwrap_or_default();
        self.last_event = match kind {
            "response.created" => {
                self.created += 1;
                "created"
            }
            "response.in_progress" => "in_progress",
            "response.output_item.added" => {
                self.items += 1;
                if matches!(
                    value.pointer("/item/type").and_then(Value::as_str),
                    Some("function_call" | "custom_tool_call" | "shell_call" | "local_shell_call")
                ) {
                    self.tool_items += 1;
                }
                if let Some(index) = value["output_index"].as_u64() {
                    if self.active_items.len() < 256 {
                        self.active_items.insert(index);
                    } else {
                        self.tracking_limited = true;
                    }
                }
                "item_added"
            }
            "response.output_item.done" => {
                if let Some(index) = value["output_index"].as_u64() {
                    self.active_items.remove(&index);
                }
                "item_done"
            }
            "response.output_text.delta"
            | "response.function_call_arguments.delta"
            | "response.custom_tool_call_input.delta" => {
                if kind == "response.output_text.delta" {
                    self.text_deltas += 1;
                } else {
                    self.tool_deltas += 1;
                }
                match value["output_index"].as_u64() {
                    Some(index)
                        if !self.tracking_limited && !self.active_items.contains(&index) =>
                    {
                        self.delta_without_item += 1
                    }
                    None => self.unindexed_deltas += 1,
                    _ => {}
                }
                if kind == "response.output_text.delta" {
                    "text_delta"
                } else {
                    "tool_delta"
                }
            }
            "response.completed" => {
                self.terminal += 1;
                "completed"
            }
            "response.incomplete" => {
                self.terminal += 1;
                "incomplete"
            }
            "response.failed" => {
                self.terminal += 1;
                "failed"
            }
            "response.cancelled" => {
                self.terminal += 1;
                "cancelled"
            }
            "error" => {
                self.terminal += 1;
                "error"
            }
            _ => "other",
        };
    }

    pub(crate) fn limited(&mut self) {
        self.tracking_limited = true;
    }

    pub(crate) fn log(&self, store: &UsageStore, request_id: Uuid, transport: &'static str) {
        let anomaly = self.delta_without_item > 0 || self.sequence_regressions > 0;
        store.log_diagnostic(if anomaly || self.terminal == 0 { "warn" } else { "info" }, format!(
            "event=proxy.responses.summary request_id={request_id} transport={transport} events={} created={} items={} tool_items={} text_deltas={} tool_deltas={} terminal={} delta_without_item={} sequence_regressions={} unindexed_deltas={} tracking_limited={} last_event={}",
            self.events, self.created, self.items, self.tool_items, self.text_deltas, self.tool_deltas,
            self.terminal, self.delta_without_item, self.sequence_regressions, self.unindexed_deltas,
            self.tracking_limited, if self.events == 0 { "none" } else { self.last_event },
        ));
    }
}

pub(crate) struct WsDiagnostic<'a> {
    pub store: &'a UsageStore,
    pub request_id: Uuid,
    pub route_id: Uuid,
    pub provider_id: Uuid,
}

impl WsDiagnostic<'_> {
    pub(crate) fn log(
        &self,
        reason: &'static str,
        status: Option<StatusCode>,
        error: Option<&(dyn std::error::Error + 'static)>,
    ) {
        let mut os_error = None;
        let mut source = error;
        // Do not format arbitrary errors: reqwest errors can contain URLs.
        for _ in 0..16 {
            let Some(err) = source else {
                break;
            };
            if let Some(io) = err.downcast_ref::<std::io::Error>() {
                os_error = io.raw_os_error();
            }
            if os_error.is_some() {
                break;
            }
            source = err.source();
        }
        self.store.log_diagnostic("info", format!(
            "event=proxy.websocket.transport request_id={} route_id={} provider_id={} reason={reason} status={:?} os_error={os_error:?}",
            self.request_id, self.route_id, self.provider_id, status.map(|status| status.as_u16()),
        ));
    }
}

#[cfg(test)]
mod tests;
