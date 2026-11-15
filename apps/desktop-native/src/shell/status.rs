use super::i18n::{tf, tr};
use aipass_agent_protocol::{ProxyStatus, SessionStatus};
pub(super) enum TrayStatus {
    Running(SessionStatus),
    Unavailable(String),
}

pub(super) struct TraySnapshot {
    pub(super) agent: TrayStatus,
    pub(super) proxy: ProxyTrayStatus,
    pub(super) routes: Vec<TrayRoute>,
    pub(super) panel: Option<aipass_agent_protocol::ControlPanelStatus>,
}

#[derive(Clone)]
pub(super) struct TrayRoute {
    pub(super) id: uuid::Uuid,
    pub(super) name: String,
    pub(super) active: bool,
}

pub(super) enum ProxyTrayStatus {
    Available(ProxyStatus),
    Unavailable(String),
}

/// Serializable status snapshot consumed by the SwiftUI tray panel.
#[derive(Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TrayStatusDto {
    pub panel_url: Option<String>,
    pub locale: aipass_agent_protocol::UiLocale,
    pub messages: std::collections::BTreeMap<String, String>,
    pub agent_text: String,
    pub agent_state: &'static str,
    pub can_start_agent: bool,
    pub can_lock: bool,
    pub proxy_text: String,
    pub proxy_state: &'static str,
    pub proxy_state_text: String,
    pub proxy_detail: Option<String>,
    pub proxy_groups: Vec<TrayGroupDto>,
    pub proxy_running: bool,
    pub can_open_proxy: bool,
    pub can_start_proxy: bool,
    pub can_stop_proxy: bool,
    pub tooltip: String,
}

#[derive(Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TrayGroupDto {
    pub(super) id: uuid::Uuid,
    pub(super) name: String,
    pub(super) active: bool,
}

impl TrayStatus {
    fn menu_text(&self) -> String {
        match self {
            Self::Running(status) if !status.exists => tr("tray.agent.noVault"),
            Self::Running(status) if status.locked => tr("tray.agent.locked"),
            Self::Running(_) => tr("tray.agent.unlocked"),
            Self::Unavailable(_) => tr("tray.agent.unavailable"),
        }
    }

    fn state_id(&self) -> &'static str {
        match self {
            Self::Running(status) if !status.exists => "no-vault",
            Self::Running(status) if status.locked => "locked",
            Self::Running(_) => "unlocked",
            Self::Unavailable(_) => "unreachable",
        }
    }

    fn tooltip(&self) -> String {
        match self {
            Self::Unavailable(err) => format!("{}: {}", self.menu_text(), short_error(err)),
            _ => self.menu_text(),
        }
    }

    pub(super) fn can_lock(&self) -> bool {
        matches!(self, Self::Running(status) if status.exists && !status.locked)
    }

    pub(super) fn can_start(&self) -> bool {
        matches!(self, Self::Unavailable(_))
    }
}

impl TraySnapshot {
    pub(super) fn unavailable(err: String) -> Self {
        Self {
            agent: TrayStatus::Unavailable(err.clone()),
            proxy: ProxyTrayStatus::Unavailable(err),
            routes: Vec::new(),
            panel: None,
        }
    }

    pub(super) fn can_start_proxy(&self) -> bool {
        // Mirror the agent-side validation: starting without an enabled route
        // group is rejected, so do not offer the action.
        matches!(
            (&self.agent, &self.proxy),
            (
                TrayStatus::Running(SessionStatus {
                    exists: true,
                    locked: false,
                    ..
                }),
                ProxyTrayStatus::Available(ProxyStatus { running: false, .. })
            )
        ) && self.routes.iter().any(|route| route.active)
    }

    pub(super) fn can_open_proxy(&self) -> bool {
        true
    }

    pub(super) fn can_stop_proxy(&self) -> bool {
        matches!(
            &self.proxy,
            ProxyTrayStatus::Available(ProxyStatus { running: true, .. })
        )
    }

    fn tooltip(&self) -> String {
        format!("{}; {}", self.agent.tooltip(), self.proxy.tooltip())
    }

    fn proxy_menu_text(&self) -> String {
        match (&self.agent, &self.proxy) {
            (
                TrayStatus::Running(SessionStatus { exists: false, .. }),
                ProxyTrayStatus::Available(ProxyStatus { running: false, .. }),
            ) => tf("tray.status", &[("status", tr("tray.noVault"))]),
            (
                TrayStatus::Running(SessionStatus { locked: true, .. }),
                ProxyTrayStatus::Available(ProxyStatus { running: false, .. }),
            ) => tf("tray.status", &[("status", tr("tray.vaultLocked"))]),
            _ => self.proxy.menu_text(),
        }
    }

    pub(super) fn dto(&self) -> TrayStatusDto {
        let (proxy_state, proxy_state_text, proxy_detail) = self.proxy_panel_fields();
        TrayStatusDto {
            panel_url: self.panel.as_ref().and_then(|panel| panel.url.clone()),
            locale: super::i18n::locale(),
            messages: super::i18n::messages(),
            agent_text: self.agent.menu_text(),
            agent_state: self.agent.state_id(),
            can_start_agent: self.agent.can_start(),
            can_lock: self.agent.can_lock(),
            proxy_text: self.proxy_menu_text(),
            proxy_state,
            proxy_state_text,
            proxy_detail,
            proxy_groups: self
                .routes
                .iter()
                .map(|route| TrayGroupDto {
                    id: route.id,
                    name: route.name.clone(),
                    active: route.active,
                })
                .collect(),
            proxy_running: matches!(
                &self.proxy,
                ProxyTrayStatus::Available(ProxyStatus { running: true, .. })
            ),
            can_open_proxy: self.can_open_proxy(),
            can_start_proxy: self.can_start_proxy(),
            can_stop_proxy: self.can_stop_proxy(),
            tooltip: self.tooltip(),
        }
    }

    fn proxy_panel_fields(&self) -> (&'static str, String, Option<String>) {
        match (&self.agent, &self.proxy) {
            (
                TrayStatus::Running(SessionStatus { exists: false, .. }),
                ProxyTrayStatus::Available(ProxyStatus { running: false, .. }),
            ) => ("no-vault", tr("tray.noVault"), None),
            (
                TrayStatus::Running(SessionStatus { locked: true, .. }),
                ProxyTrayStatus::Available(ProxyStatus { running: false, .. }),
            ) => ("locked", tr("tray.vaultLocked"), None),
            (
                _,
                ProxyTrayStatus::Available(ProxyStatus {
                    running: true,
                    bind_addr,
                    active_routes,
                    ..
                }),
            ) => (
                "running",
                tr("server.running"),
                Some(format!("{bind_addr}  {}", route_count(*active_routes))),
            ),
            (_, ProxyTrayStatus::Available(_)) => ("stopped", tr("server.stopped"), None),
            (_, ProxyTrayStatus::Unavailable(_)) => ("unavailable", tr("tray.unavailable"), None),
        }
    }
}

impl ProxyTrayStatus {
    fn menu_text(&self) -> String {
        let status = match self {
            Self::Available(status) if status.running => format!(
                "{} | {} | {}",
                tr("server.running"),
                status.bind_addr,
                route_count(status.active_routes)
            ),
            Self::Available(_) => tr("server.stopped"),
            Self::Unavailable(_) => tr("tray.unavailable"),
        };
        tf("tray.status", &[("status", status)])
    }

    fn tooltip(&self) -> String {
        match self {
            Self::Unavailable(err) => format!("{}: {}", self.menu_text(), short_error(err)),
            _ => self.menu_text(),
        }
    }
}

fn route_count(count: usize) -> String {
    tf(
        if count == 1 {
            "tray.routes.one"
        } else {
            "tray.routes.other"
        },
        &[("count", count.to_string())],
    )
}

fn short_error(value: &str) -> String {
    let line = value.lines().next().unwrap_or(value).trim();
    const MAX_LEN: usize = 160;
    let mut chars = line.chars();
    let shortened = chars.by_ref().take(MAX_LEN).collect::<String>();
    if chars.next().is_some() {
        format!("{shortened}...")
    } else {
        shortened
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aipass_agent_protocol::SessionPolicy;

    #[test]
    fn proxy_status_text_includes_bind_address_and_routes_when_running() {
        let snapshot = snapshot(false, available_proxy(true, 3));

        assert_eq!(
            snapshot.proxy_menu_text(),
            "Status: Running | 127.0.0.1:8787 | 3 routes"
        );
        assert!(snapshot.tooltip().contains("127.0.0.1:8787"));
        assert!(snapshot.tooltip().contains("3 routes"));
    }

    #[test]
    fn proxy_status_text_distinguishes_stopped_locked_and_unavailable() {
        assert_eq!(
            snapshot(false, available_proxy(false, 0)).proxy_menu_text(),
            "Status: Stopped"
        );
        assert_eq!(
            snapshot(true, available_proxy(false, 0)).proxy_menu_text(),
            "Status: Vault locked"
        );
        let mut no_vault = snapshot(true, available_proxy(false, 0));
        if let TrayStatus::Running(status) = &mut no_vault.agent {
            status.exists = false;
        }
        assert_eq!(no_vault.proxy_menu_text(), "Status: No vault");
        assert_eq!(
            snapshot(
                false,
                ProxyTrayStatus::Unavailable("agent error".to_string())
            )
            .proxy_menu_text(),
            "Status: Unavailable"
        );
    }

    #[test]
    fn proxy_actions_respect_vault_and_runtime_state() {
        let stopped = snapshot(false, available_proxy(false, 1));
        assert!(stopped.can_open_proxy());
        assert!(stopped.can_start_proxy());
        assert!(!stopped.can_stop_proxy());

        let locked = snapshot(true, available_proxy(false, 1));
        assert!(!locked.can_start_proxy());
        assert!(!locked.can_stop_proxy());

        let locked_running = snapshot(true, available_proxy(true, 1));
        assert!(!locked_running.can_start_proxy());
        assert!(locked_running.can_stop_proxy());

        let unavailable = TraySnapshot::unavailable("offline".to_string());
        assert!(unavailable.can_open_proxy());
        assert!(!unavailable.can_start_proxy());
        assert!(!unavailable.can_stop_proxy());
    }

    #[test]
    fn proxy_start_requires_an_enabled_route_group() {
        // No route groups at all: nothing to serve.
        let mut no_routes = snapshot(false, available_proxy(false, 0));
        no_routes.routes.clear();
        assert!(!no_routes.can_start_proxy());

        // Route groups exist but every one is disabled: the agent would
        // reject the start, so the tray must not offer it.
        let mut all_inactive = snapshot(false, available_proxy(false, 0));
        all_inactive.routes[0].active = false;
        assert!(!all_inactive.can_start_proxy());

        let one_active = snapshot(false, available_proxy(false, 0));
        assert!(one_active.can_start_proxy());
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn dto_mirrors_snapshot_state() {
        let running = snapshot(false, available_proxy(true, 2)).dto();
        assert_eq!(running.agent_state, "unlocked");
        assert_eq!(running.proxy_state, "running");
        assert_eq!(
            running.proxy_detail.as_deref(),
            Some("127.0.0.1:8787  2 routes")
        );
        assert!(running.proxy_running);
        assert!(running.can_lock);
        assert!(!running.can_start_agent);
        assert!(running.can_stop_proxy);
        assert!(!running.can_start_proxy);

        let locked = snapshot(true, available_proxy(false, 0)).dto();
        assert_eq!(locked.agent_state, "locked");
        assert_eq!(locked.proxy_state, "locked");
        assert_eq!(locked.proxy_state_text, "Vault locked");
        assert!(!locked.can_lock);
        assert!(!locked.can_start_proxy);

        let mut no_vault = snapshot(true, available_proxy(false, 0));
        if let TrayStatus::Running(status) = &mut no_vault.agent {
            status.exists = false;
        }
        let no_vault = no_vault.dto();
        assert_eq!(no_vault.agent_state, "no-vault");
        assert_eq!(no_vault.proxy_state, "no-vault");

        let offline = TraySnapshot::unavailable("offline".to_string()).dto();
        assert_eq!(offline.agent_state, "unreachable");
        assert_eq!(offline.proxy_state, "unavailable");
        assert!(offline.can_start_agent);
        assert!(!offline.can_stop_proxy);
    }

    fn snapshot(locked: bool, proxy: ProxyTrayStatus) -> TraySnapshot {
        TraySnapshot {
            panel: None,
            agent: TrayStatus::Running(SessionStatus {
                exists: true,
                locked,
                policy: SessionPolicy::default(),
                last_lock_reason: None,
                vault_namespace: Some("test".to_string()),
                initial_sync_pending: false,
                initial_sync_failed: false,
                sync_revision: 0,
                sync_status: None,
            }),
            proxy,
            routes: vec![TrayRoute {
                id: uuid::Uuid::new_v4(),
                name: "default".to_string(),
                active: true,
            }],
        }
    }

    fn available_proxy(running: bool, active_routes: usize) -> ProxyTrayStatus {
        ProxyTrayStatus::Available(ProxyStatus {
            running,
            enabled: running,
            bind_addr: "127.0.0.1:8787".to_string(),
            active_routes,
            requests: 0,
            failures: 0,
            last_error: None,
            degraded: false,
            degraded_target_ids: Vec::new(),
            channels: Vec::new(),
            recent_requests: 0,
            recent_tokens: 0,
            success_rate_bps: 0,
            average_first_token_ms: None,
            in_flight_requests: 0,
            available_channels: active_routes,
            total_channels: active_routes,
        })
    }
}
