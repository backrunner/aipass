//! Archive completed, text-only CLI sessions through encrypted Agent storage.
//! The archive is an opaque vendor transcript, never reconstructed tool output.
use super::*;
use aipass_agent_protocol::{ClaudeSessionCheckpoint, ClaudeSessionOperation};
use std::io::Read;
const MAX_TRANSCRIPT: u64 = 4 * 1024 * 1024;

pub(super) fn workspace(vault: &Path) -> Result<PathBuf, String> {
    let path = vault.join("claude-workspace");
    std::fs::create_dir_all(&path).map_err(|_| "Claude workspace unavailable")?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700))
            .map_err(|_| "Cannot protect Claude workspace")?;
    }
    Ok(path)
}
pub(super) fn load(
    inner: &Inner,
    target: &ResolvedTarget,
    context: [u8; 32],
    history: [u8; 32],
) -> Option<ClaudeSessionCheckpoint> {
    let present = inner.runs.lock().ok()?.values().any(|r| {
        r.owner == target.config.id
            && r.context == context
            && r.history == history
            && r.fingerprint == Sha256::digest(target.api_key.as_bytes()).as_slice()
    });
    if present {
        return None;
    }
    crate::AgentClient::for_vault(inner.vault.clone())
        .ok()?
        .request(&AgentRequest::ClaudeNativeSession {
            entry_id: target.config.provider_entry_id,
            marker: SensitiveString::new(&target.api_key),
            operation: ClaudeSessionOperation::Load {
                owner: target.config.id,
                context,
                history,
            },
        })
        .ok()
        .flatten()
}

fn bytes(path: &Path) -> Option<zeroize::Zeroizing<Vec<u8>>> {
    let meta = std::fs::symlink_metadata(path).ok()?;
    if !meta.is_file() || meta.len() > MAX_TRANSCRIPT {
        return None;
    }
    let mut bytes = zeroize::Zeroizing::new(Vec::new());
    std::fs::File::open(path)
        .ok()?
        .take(MAX_TRANSCRIPT + 1)
        .read_to_end(&mut bytes)
        .ok()?;
    (bytes.len() as u64 <= MAX_TRANSCRIPT).then_some(bytes)
}
pub(super) fn restore(
    checkpoint: &ClaudeSessionCheckpoint,
    home: &Path,
    workspace: &Path,
) -> Option<Uuid> {
    if checkpoint.workspace != workspace
        || checkpoint.project_key.is_empty()
        || !checkpoint
            .project_key
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"_-".contains(&b))
        || !complete(checkpoint.transcript.expose(), None)
    {
        return None;
    }
    let project = home.join("projects").join(&checkpoint.project_key);
    std::fs::create_dir_all(&project).ok()?;
    let file = project.join(format!("{}.jsonl", checkpoint.native_session));
    if let Some(existing) = bytes(&file) {
        // A changed vendor transcript may belong to an external CLI. Never
        // rewind or overwrite it to satisfy an old proxy conversation.
        if existing.as_slice() != checkpoint.transcript.expose().as_bytes() {
            return None;
        }
    } else {
        write_private(&file, checkpoint.transcript.expose().as_bytes()).ok()?;
    }
    Some(checkpoint.native_session)
}
fn complete(raw: &str, message: Option<&str>) -> bool {
    let mut last = None;
    for line in raw.lines().filter(|s| !s.trim().is_empty()) {
        let Ok(value) = serde_json::from_str::<Value>(line) else {
            return false;
        };
        if matches!(value["type"].as_str(), Some("assistant" | "user")) {
            last = Some(value);
        }
    }
    last.is_some_and(|v| {
        v["type"] == "assistant"
            && v["message"]["stop_reason"] == "end_turn"
            && message.is_none_or(|id| v["message"]["id"] == id)
    })
}
fn text_only(value: &Value) -> bool {
    match value {
        Value::Array(values) => values.iter().all(text_only),
        Value::Object(values) => {
            !matches!(
                values.get("type").and_then(Value::as_str),
                Some("image" | "document")
            ) && values.values().all(text_only)
        }
        _ => true,
    }
}
pub(super) fn save(inner: &Inner, key: &str) {
    let prepared = inner.runs.lock().ok().and_then(|runs| {
        let run = runs.get(key)?;
        if !run.terminal || run.busy() || !text_only(&json!(run.history_messages)) {
            return None;
        }
        Some((
            run.entry_id,
            run.marker.clone(),
            run.owner,
            run.context,
            run.history,
            run.native_session,
            run.cli_home.clone()?,
            run.workspace.clone(),
            run.native_message.clone()?,
            aipass_proxy::subscription_history_call_ids(&json!({"messages":run.history_messages}))
                .into_iter()
                .filter(|id| id.starts_with("toolu_aipass_claude_"))
                .map(str::to_owned)
                .collect::<Vec<_>>(),
        ))
    });
    let Some((id, marker, owner, context, history, session, home, workspace, message, call_ids)) =
        prepared
    else {
        return;
    };
    let Ok(projects) = std::fs::read_dir(home.join("projects")) else {
        return;
    };
    for entry in projects.take(512).flatten() {
        if !entry.file_type().is_ok_and(|t| t.is_dir()) {
            continue;
        }
        let path = entry.path().join(format!("{session}.jsonl"));
        let Some(raw) = bytes(&path) else { continue };
        let Ok(transcript) = std::str::from_utf8(&raw) else {
            return;
        };
        if !complete(transcript, Some(&message)) {
            continue;
        }
        let Some(project_key) = entry.file_name().to_str().map(str::to_owned) else {
            return;
        };
        let checkpoint = ClaudeSessionCheckpoint {
            owner,
            context,
            history,
            native_session: session,
            project_key,
            workspace,
            call_ids,
            transcript: SensitiveString::new(transcript),
            observed_at: time::OffsetDateTime::now_utc().unix_timestamp(),
        };
        if let Ok(client) = crate::AgentClient::for_vault(inner.vault.clone()) {
            let _ = client.request::<Option<ClaudeSessionCheckpoint>>(
                &AgentRequest::ClaudeNativeSession {
                    entry_id: id,
                    marker,
                    operation: ClaudeSessionOperation::Save { checkpoint },
                },
            );
        }
        return;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn only_complete_text_turns_are_recoverable_and_external_transcripts_win() {
        let home = tempfile::tempdir().unwrap();
        let workspace = home.path().join("workspace");
        let checkpoint = ClaudeSessionCheckpoint {
            owner: Uuid::new_v4(),
            context: [1; 32],
            history: [2; 32],
            native_session: Uuid::new_v4(),
            project_key: "-private-project".into(),
            workspace: workspace.clone(),
            call_ids: Vec::new(),
            transcript: SensitiveString::new(
                json!({"type":"assistant","message":{"id":"msg","stop_reason":"end_turn"}})
                    .to_string(),
            ),
            observed_at: time::OffsetDateTime::now_utc().unix_timestamp(),
        };
        assert!(restore(&checkpoint, home.path(), &workspace).is_some());
        let file = home
            .path()
            .join("projects")
            .join(&checkpoint.project_key)
            .join(format!("{}.jsonl", checkpoint.native_session));
        std::fs::write(&file, "changed by an external CLI").unwrap();
        assert!(restore(&checkpoint, home.path(), &workspace).is_none());
        assert_eq!(
            std::fs::read_to_string(file).unwrap(),
            "changed by an external CLI"
        );
        assert!(!complete(
            &json!({"type":"assistant","message":{"stop_reason":"tool_use"}}).to_string(),
            None
        ));
        assert!(!text_only(&json!([{"content":[{"type":"image"}]}])));
    }
}
