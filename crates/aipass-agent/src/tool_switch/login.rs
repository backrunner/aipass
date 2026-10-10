//! Target-bound official login tickets. Browser interaction remains user initiated.
use super::*;
use std::{
    collections::HashMap,
    sync::{LazyLock, Once, Weak},
    time::{Duration, Instant},
};
struct Login {
    owner: Weak<AgentState>,
    vault_dir: PathBuf,
    session_id: Uuid,
    request: ToolConfigRequest,
    created: Instant,
    config_resources: Vec<Resource>,
    fingerprint: String,
    result: Option<ToolConfigApplyResponse>,
}
static LOGINS: LazyLock<Mutex<HashMap<Uuid, Login>>> = LazyLock::new(|| Mutex::new(HashMap::new()));
const TTL: Duration = Duration::from_secs(900);
fn expire(state: &Arc<AgentState>) {
    let tickets = LOGINS
        .lock()
        .map(|logins| {
            logins
                .iter()
                .filter(|(_, l)| l.vault_dir == state.vault_dir && l.created.elapsed() >= TTL)
                .map(|(id, _)| *id)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    for ticket in tickets {
        cancel(state, ticket);
    }
}
fn start_sweeper() {
    static START: Once = Once::new();
    START.call_once(|| {
        std::thread::spawn(|| loop {
            std::thread::sleep(Duration::from_secs(15));
            let owners = LOGINS
                .lock()
                .map(|mut logins| {
                    logins.retain(|_, l| l.owner.strong_count() > 0);
                    logins
                        .values()
                        .filter(|l| l.created.elapsed() >= TTL)
                        .filter_map(|l| l.owner.upgrade())
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();
            for state in owners {
                expire(&state);
            }
        });
    });
}
fn bridge(
    state: &Arc<AgentState>,
) -> ServiceResult<(
    Arc<crate::community::CommunityBridge>,
    aipass_proxy::UpstreamProxyConfig,
)> {
    with_vault(state, false, |vault| {
        let mut proxy = state
            .proxy
            .lock()
            .map_err(|_| safe_error(anyhow::anyhow!("proxy unavailable")))?;
        let config = proxy.load_config(vault)?;
        Ok((proxy.community_bridge(), config.upstream_proxy))
    })
}
pub(crate) fn handle_login(
    state: &Arc<AgentState>,
    request: AgentRequest,
) -> ServiceResult<AgentResponse> {
    expire(state);
    match request {
        AgentRequest::ToolConfigLoginStart { request } => {
            start(state, request).map(AgentResponse::success)
        }
        AgentRequest::ToolConfigLoginPoll { ticket } => {
            poll(state, ticket).map(AgentResponse::success)
        }
        AgentRequest::ToolConfigLoginCancel { ticket } => {
            cancel(state, ticket);
            Ok(AgentResponse::empty())
        }
        AgentRequest::ToolConfigLoginCode { ticket, code } => {
            with_vault(state, false, |_| Ok(()))?;
            let logins = LOGINS
                .lock()
                .map_err(|_| safe_error(anyhow::anyhow!("login coordinator unavailable")))?;
            let login = logins
                .get(&ticket)
                .ok_or_else(|| safe_error(anyhow::anyhow!("sign-in expired")))?;
            if login.vault_dir != state.vault_dir {
                return Err(safe_error(anyhow::anyhow!("sign-in expired")));
            }
            if login.request.tool == ToolConfigTool::ClaudeCode {
                crate::claude_cli::logins()
                    .code(ticket, code.expose())
                    .map_err(|e| safe_error(anyhow::anyhow!(e)))?;
            } else {
                bridge(state)?
                    .0
                    .login_code(ticket, code.expose())
                    .map_err(|e| safe_error(anyhow::anyhow!(e)))?;
            }
            Ok(AgentResponse::empty())
        }
        _ => Err(safe_error(anyhow::anyhow!("unsupported sign-in operation"))),
    }
}
fn session_id(state: &Arc<AgentState>) -> ServiceResult<Uuid> {
    match &*state
        .session
        .lock()
        .map_err(|_| safe_error(anyhow::anyhow!("session unavailable")))?
    {
        crate::session::SessionState::Unlocked(info) => Ok(info.id),
        _ => Err(ServiceError::new(
            AgentErrorCode::Locked,
            "vault locked; sign-in cancelled",
        )),
    }
}
fn start(
    state: &Arc<AgentState>,
    request: ToolConfigRequest,
) -> ServiceResult<ToolConfigLoginStatus> {
    if request.mode != ToolConfigMode::Official || !supported(&request.tool) {
        return Err(safe_error(anyhow::anyhow!(
            "reauthentication requires a native subscription"
        )));
    }
    static START_LOCK: Mutex<()> = Mutex::new(());
    let _start_guard = START_LOCK
        .lock()
        .map_err(|_| safe_error(anyhow::anyhow!("login coordinator unavailable")))?;
    start_sweeper();
    expire(state);
    if LOGINS
        .lock()
        .map_err(|_| safe_error(anyhow::anyhow!("login coordinator unavailable")))?
        .values()
        .any(|l| {
            l.vault_dir == state.vault_dir && l.request.tool == request.tool && l.result.is_none()
        })
    {
        return Err(ServiceError::new(
            AgentErrorCode::Conflict,
            "A sign-in task is already running for this tool; cancel it before retrying",
        ));
    }
    let original_session = session_id(state)?;
    let (resources, stamp) = with_vault(state, false, |vault| {
        let (_, plan, _) = crate::server::build_tool_config_plan(vault, &request, true)?;
        if request.preview_id.as_ref().is_some_and(|id| {
            preview_id(vault, state, &request, &plan).map_or(true, |actual| actual != *id)
        }) {
            return Err(ServiceError::new(
                AgentErrorCode::Conflict,
                "tool credentials changed; preview again",
            ));
        }
        let home = crate::server::home_dir(vault)?;
        let mut resources = active_store(&home, &request.tool)
            .map_err(safe_error)?
            .resources();
        resources.push(Resource::File(plan.target_path));
        resources.push(Resource::File(binding_path(state, &request.tool)));
        let stamp = fingerprint(
            &resources,
            &vault.config_backup_key(),
            &context(&request).map_err(safe_error)?,
        )
        .map_err(safe_error)?;
        Ok((resources, stamp))
    })?;
    let ticket = if request.tool == ToolConfigTool::ClaudeCode {
        let epoch = with_vault(state, false, |_| Ok(crate::claude_cli::logins().epoch()))?;
        crate::claude_cli::logins()
            .start(epoch)
            .map_err(|e| safe_error(anyhow::anyhow!(e)))?
            .ticket
    } else {
        let (bridge, proxy) = bridge(state)?;
        bridge
            .login_start_bound(
                state,
                CommunityLoginInput {
                    provider: "codex".into(),
                    // Method 0 starts an isolated official login; method 1 imports
                    // the current CLI home and cannot repair an expired account.
                    method: 0,
                    inputs: Default::default(),
                    api_key: None,
                },
                proxy,
                Some(request.id),
            )
            .map_err(|e| safe_error(anyhow::anyhow!(e)))?
            .ticket
    };
    let mut logins = LOGINS
        .lock()
        .map_err(|_| safe_error(anyhow::anyhow!("login coordinator unavailable")))?;
    logins.insert(
        ticket,
        Login {
            owner: Arc::downgrade(state),
            vault_dir: state.vault_dir.clone(),
            session_id: original_session,
            request,
            created: Instant::now(),
            config_resources: resources,
            fingerprint: stamp,
            result: None,
        },
    );
    drop(logins);
    if session_id(state).ok() != Some(original_session) {
        cancel(state, ticket);
        return Err(ServiceError::new(
            AgentErrorCode::Locked,
            "vault locked; sign-in cancelled",
        ));
    }
    Ok(ToolConfigLoginStatus {
        ticket,
        status: "pending".into(),
        url: None,
        user_code: None,
        requires_code: false,
        message: None,
        result: None,
    })
}
fn poll(state: &Arc<AgentState>, ticket: Uuid) -> ServiceResult<ToolConfigLoginStatus> {
    expire(state);
    let current_session = session_id(state)?;
    let mut logins = LOGINS
        .lock()
        .map_err(|_| safe_error(anyhow::anyhow!("login coordinator unavailable")))?;
    let login = logins
        .get_mut(&ticket)
        .ok_or_else(|| safe_error(anyhow::anyhow!("sign-in expired")))?;
    if login.vault_dir != state.vault_dir
        || login.session_id != current_session
        || login.created.elapsed() > Duration::from_secs(900)
    {
        return Err(safe_error(anyhow::anyhow!("sign-in expired")));
    }
    if let Some(result) = &login.result {
        return Ok(ToolConfigLoginStatus {
            ticket,
            status: "complete".into(),
            url: None,
            user_code: None,
            requires_code: false,
            message: None,
            result: Some(result.clone()),
        });
    }
    let mut response = ToolConfigLoginStatus {
        ticket,
        status: "pending".into(),
        url: None,
        user_code: None,
        requires_code: login.request.tool == ToolConfigTool::ClaudeCode,
        message: None,
        result: None,
    };
    let complete = if login.request.tool == ToolConfigTool::ClaudeCode {
        let status = crate::claude_cli::logins()
            .poll(ticket)
            .map_err(|e| safe_error(anyhow::anyhow!(e)))?;
        response.url = status.url;
        response.message = status.message;
        if status.status == "authorized" {
            with_vault(state, false, |vault| {
                crate::claude_cli::logins().commit(ticket, |account| {
                    bind_claude(state, vault, login.request.id, account)
                })
            })?;
            true
        } else {
            if status.status != "pending" {
                response.status = status.status;
            }
            false
        }
    } else {
        let status = bridge(state)?
            .0
            .login_poll(ticket)
            .map_err(|e| safe_error(anyhow::anyhow!(e)))?;
        response.url = status.url;
        response.user_code = status.user_code;
        response.message = status.error;
        if status.status == "complete" {
            true
        } else {
            response.status = status.status;
            false
        }
    };
    if complete {
        // Keep the final configuration check and continuation in one switch transaction.
        let _guard = SWITCH_LOCK
            .lock()
            .map_err(|_| safe_error(anyhow::anyhow!("tool coordinator unavailable")))?;
        with_vault(state, false, |vault| {
            let actual = fingerprint(
                &login.config_resources,
                &vault.config_backup_key(),
                &context(&login.request).map_err(safe_error)?,
            )
            .map_err(safe_error)?;
            if actual != login.fingerprint {
                return Err(ServiceError::new(
                    AgentErrorCode::Conflict,
                    "tool credentials changed during sign-in; preview again",
                ));
            }
            Ok(())
        })?;
        let mut request = login.request.clone();
        request.preview_id = None;
        let result = apply_locked(state, request.clone())
            .or_else(|error| safety::apply_error(&request, error))?;
        if result.outcome == ToolConfigOutcome::Applied {
            response.status = "complete".into();
        } else {
            response.status = "failed".into();
            response.message = result.message.clone();
        }
        response.result = Some(result.clone());
        login.result = Some(result);
    }
    Ok(response)
}
fn cancel(state: &Arc<AgentState>, ticket: Uuid) {
    let login = LOGINS.lock().ok().and_then(|mut l| {
        if l.get(&ticket)
            .is_some_and(|login| login.vault_dir == state.vault_dir)
        {
            l.remove(&ticket)
        } else {
            None
        }
    });
    if let Some(login) = login {
        if login.request.tool == ToolConfigTool::ClaudeCode {
            crate::claude_cli::logins().cancel(ticket);
        } else if let Ok(proxy) = state.proxy.lock() {
            let _ = proxy.community_bridge().login_cancel(ticket);
        }
    }
}

pub(crate) fn clear(state: &Arc<AgentState>) {
    let tickets = LOGINS
        .lock()
        .map(|l| {
            l.iter()
                .filter(|(_, l)| l.vault_dir == state.vault_dir)
                .map(|(id, _)| *id)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    for ticket in tickets {
        cancel(state, ticket);
    }
}

pub(crate) fn bind_codex(vault: &Vault, id: Uuid, auth: Value) -> ServiceResult<()> {
    let entry = vault.get_provider_summary(id).map_err(map_vault_error)?;
    if !matches!(entry.provider_id.as_deref(), Some("codex" | "openai"))
        || entry.provider_kind != aipass_provider_registry::ProviderKind::Official
        || entry.archived_at.is_some()
        || entry.deleted_at.is_some()
        || entry.credential_kind != aipass_provider_registry::CredentialKind::OAuth
        || entry.account_identity != crate::community::identity(&auth)
    {
        return Err(ServiceError::new(
            AgentErrorCode::Conflict,
            "Signed-in account or workspace does not match the selected subscription",
        ));
    }
    crate::community::bind_cli(vault, id, "codex", auth)
}
pub(super) fn bind_claude(
    state: &Arc<AgentState>,
    vault: &Vault,
    id: Uuid,
    account: &crate::claude_cli::NativeAccount,
) -> ServiceResult<Uuid> {
    let entry = vault.get_provider_summary(id).map_err(map_vault_error)?;
    if entry.provider_id.as_deref() != Some("anthropic")
        || entry.provider_kind != aipass_provider_registry::ProviderKind::Official
        || entry.credential_kind != aipass_provider_registry::CredentialKind::OAuth
        || entry.archived_at.is_some()
        || entry.deleted_at.is_some()
        || entry.account_identity.as_deref() != Some(account.identity.as_str())
    {
        return Err(ServiceError::new(
            AgentErrorCode::Conflict,
            "Signed-in account does not match the selected subscription",
        ));
    }
    let auth = SecretString::new(json!({"nativeHome":account.home,"accountId":account.identity,"nativeDevice":crate::subscriptions::cli_accounts::device().map_err(|e| safe_error(anyhow::anyhow!(e)))?}).to_string());
    vault
        .bind_cli_subscription(
            id,
            "anthropic",
            &account.identity,
            &format!("aipass:claude-cli:{}", Uuid::new_v4()),
            "claude_cli_home",
            &auth,
        )
        .map_err(map_vault_error)?;
    state
        .proxy
        .lock()
        .map_err(|_| safe_error(anyhow::anyhow!("proxy unavailable")))?
        .refresh_provider_credentials(vault, id)?;
    Ok(id)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn tickets_are_vault_and_session_bound_and_lock_cancels_them() {
        let (_dir, state, _) = super::super::tests::fixture();
        let (_other_dir, other, _) = super::super::tests::fixture();
        let ticket = Uuid::new_v4();
        LOGINS.lock().unwrap().insert(
            ticket,
            Login {
                owner: Arc::downgrade(&state),
                vault_dir: state.vault_dir.clone(),
                session_id: session_id(&state).unwrap(),
                created: Instant::now(),
                request: super::super::tests::request(Uuid::new_v4(), ToolConfigMode::Official),
                config_resources: Vec::new(),
                fingerprint: String::new(),
                result: None,
            },
        );
        assert_eq!(
            start(
                &state,
                super::super::tests::request(Uuid::new_v4(), ToolConfigMode::Official)
            )
            .unwrap_err()
            .code,
            AgentErrorCode::Conflict
        );
        cancel(&other, ticket);
        assert!(LOGINS.lock().unwrap().contains_key(&ticket));
        assert!(poll(&other, ticket).is_err());
        crate::session::lock_session(&state, LockReason::Manual);
        assert!(!LOGINS.lock().unwrap().contains_key(&ticket));
        assert!(poll(&state, ticket).is_err());
    }
    #[test]
    fn expired_tickets_are_cancelled_without_waiting_for_another_login() {
        let (_dir, state, _) = super::super::tests::fixture();
        let ticket = Uuid::new_v4();
        LOGINS.lock().unwrap().insert(
            ticket,
            Login {
                owner: Arc::downgrade(&state),
                vault_dir: state.vault_dir.clone(),
                session_id: session_id(&state).unwrap(),
                created: Instant::now() - TTL,
                request: super::super::tests::request(Uuid::new_v4(), ToolConfigMode::Official),
                config_resources: Vec::new(),
                fingerprint: String::new(),
                result: None,
            },
        );
        assert!(poll(&state, ticket).is_err());
        assert!(!LOGINS.lock().unwrap().contains_key(&ticket));
    }
}
