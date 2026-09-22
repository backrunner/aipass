use crate::session::{AgentState, ServiceError, ServiceResult, SessionInfo};
use aipass_agent_protocol::{AgentErrorCode, SensitiveString};
use rand_core::{OsRng, RngCore};
use sha2::{Digest, Sha256};
use std::{
    cell::RefCell,
    collections::{HashMap, VecDeque},
    net::IpAddr,
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        Arc, Mutex,
    },
    time::{Duration, Instant},
};
use uuid::Uuid;

pub(super) fn cookie_name(https: bool, port: u16) -> String {
    // Cookies are scoped by host, not port; keep multiple local vaults independent.
    format!("{}aipass-panel-{port}", if https { "__Host-" } else { "" })
}
const IDLE: Duration = Duration::from_secs(15 * 60);
const LIFETIME: Duration = Duration::from_secs(8 * 60 * 60);

#[derive(Clone)]
pub(crate) struct Binding {
    session_id: Uuid,
    password_revision: [u8; 32],
}

impl Binding {
    pub(crate) fn from_session(info: &SessionInfo) -> Self {
        Self {
            session_id: info.id,
            password_revision: info.vault.password_revision(),
        }
    }

    fn matches(&self, info: &SessionInfo) -> bool {
        self.session_id == info.id && self.password_revision == info.vault.password_revision()
    }
}

#[derive(Clone)]
struct Authorization {
    binding: Binding,
    expires: Instant,
    revoked: Arc<AtomicBool>,
    generation: Arc<AtomicU64>,
    issued_generation: u64,
}

thread_local! {
    static REQUEST: RefCell<Option<Authorization>> = const { RefCell::new(None) };
}

// Every vault access rechecks authorization while holding the session mutex.
// Local lock/unlock, password changes and sync cannot revive an old HTTP request.
pub(crate) fn check_session(info: &SessionInfo) -> ServiceResult<()> {
    REQUEST.with(|request| {
        if let Some(auth) = request.borrow().as_ref() {
            if auth.revoked.load(Ordering::Acquire)
                || auth.generation.load(Ordering::Acquire) != auth.issued_generation
                || Instant::now() >= auth.expires
                || !auth.binding.matches(info)
            {
                return Err(denied());
            }
        }
        Ok(())
    })
}

pub(crate) fn remote_request() -> bool {
    REQUEST.with(|request| request.borrow().is_some())
}

pub(super) fn denied() -> ServiceError {
    ServiceError::new(AgentErrorCode::PermissionDenied, "Sign in again.")
}

struct Session {
    auth: Authorization,
    csrf: String,
    last_activity: Instant,
    preview: Option<super::api::Preview>,
}

#[derive(Default)]
pub(super) struct Sessions {
    sessions: Mutex<HashMap<[u8; 32], Session>>,
    attempts: Mutex<VecDeque<(IpAddr, Instant)>>,
    login_busy: AtomicBool,
    generation: Arc<AtomicU64>,
}

pub(super) struct LoginGuard<'a>(&'a AtomicBool);
impl Drop for LoginGuard<'_> {
    fn drop(&mut self) {
        self.0.store(false, Ordering::Release);
    }
}

impl Sessions {
    pub fn login_permit(&self, ip: IpAddr) -> Option<LoginGuard<'_>> {
        let mut attempts = self.attempts.lock().ok()?;
        let now = Instant::now();
        attempts.retain(|(_, time)| now.duration_since(*time) < Duration::from_secs(60));
        if attempts.len() >= 20 || attempts.iter().filter(|(peer, _)| *peer == ip).count() >= 5 {
            return None;
        }
        if self
            .login_busy
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
        {
            return None;
        }
        attempts.push_back((ip, now));
        Some(LoginGuard(&self.login_busy))
    }

    pub fn generation(&self) -> u64 {
        self.generation.load(Ordering::Acquire)
    }

    pub fn issue(
        &self,
        binding: Binding,
        generation: u64,
    ) -> ServiceResult<(SensitiveString, String)> {
        let token = random_token();
        let csrf = random_token();
        let now = Instant::now();
        let mut sessions = self.sessions.lock().map_err(|_| super::unavailable())?;
        if generation != self.generation() {
            return Err(denied());
        }
        sessions.retain(|_, session| {
            session.auth.expires > now && now.duration_since(session.last_activity) < IDLE
        });
        if sessions.len() >= 32 {
            return Err(ServiceError::new(
                AgentErrorCode::ServiceUnavailable,
                "Too many active panel sessions.",
            ));
        }
        sessions.insert(
            digest(&token),
            Session {
                auth: Authorization {
                    binding,
                    expires: now + LIFETIME,
                    revoked: Arc::new(AtomicBool::new(false)),
                    generation: self.generation.clone(),
                    issued_generation: generation,
                },
                csrf: csrf.clone(),
                last_activity: now,
                preview: None,
            },
        );
        Ok((token.into(), csrf))
    }

    pub fn authorized<T>(
        &self,
        token: &str,
        csrf: Option<&str>,
        action: impl FnOnce() -> ServiceResult<T>,
    ) -> ServiceResult<T> {
        let auth = {
            let mut sessions = self.sessions.lock().map_err(|_| super::unavailable())?;
            let session = sessions.get_mut(&digest(token)).ok_or_else(denied)?;
            let now = Instant::now();
            if now >= session.auth.expires
                || now.duration_since(session.last_activity) >= IDLE
                || session.auth.revoked.load(Ordering::Acquire)
            {
                return Err(denied());
            }
            if let Some(csrf) = csrf {
                if digest(csrf)
                    .iter()
                    .zip(digest(&session.csrf))
                    .fold(0_u8, |v, (a, b)| v | (a ^ b))
                    != 0
                {
                    return Err(denied());
                }
                session.last_activity = now;
            }
            let mut auth = session.auth.clone();
            auth.expires = auth.expires.min(session.last_activity + IDLE);
            auth
        };
        struct Guard;
        impl Drop for Guard {
            fn drop(&mut self) {
                REQUEST.with(|r| *r.borrow_mut() = None);
            }
        }
        REQUEST.with(|r| *r.borrow_mut() = Some(auth));
        let _guard = Guard;
        action()
    }

    pub fn csrf(&self, token: &str) -> ServiceResult<String> {
        self.sessions
            .lock()
            .map_err(|_| super::unavailable())?
            .get(&digest(token))
            .map(|s| s.csrf.clone())
            .ok_or_else(denied)
    }

    pub fn logout(&self, token: &str) {
        if let Ok(mut sessions) = self.sessions.lock() {
            if let Some(session) = sessions.remove(&digest(token)) {
                session.auth.revoked.store(true, Ordering::Release);
            }
        }
    }

    pub fn revoke_all(&self) {
        if let Ok(mut sessions) = self.sessions.lock() {
            self.generation.fetch_add(1, Ordering::AcqRel);
            for session in sessions.values() {
                session.auth.revoked.store(true, Ordering::Release);
            }
            sessions.clear();
        }
    }

    pub fn save_preview(&self, token: &str, preview: super::api::Preview) -> ServiceResult<()> {
        self.sessions
            .lock()
            .map_err(|_| super::unavailable())?
            .get_mut(&digest(token))
            .ok_or_else(denied)?
            .preview = Some(preview);
        Ok(())
    }

    pub fn take_preview(&self, token: &str, id: Uuid) -> ServiceResult<super::api::Preview> {
        self.sessions
            .lock()
            .map_err(|_| super::unavailable())?
            .get_mut(&digest(token))
            .ok_or_else(denied)?
            .preview
            .take()
            .filter(|p| p.id == id && p.created.elapsed() < Duration::from_secs(300))
            .ok_or_else(|| super::invalid("Preview expired. Preview the change again."))
    }
}

pub(super) fn random_token() -> String {
    let mut bytes = [0_u8; 32];
    OsRng.fill_bytes(&mut bytes);
    use base64::Engine;
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes)
}

fn digest(token: &str) -> [u8; 32] {
    Sha256::digest(token.as_bytes()).into()
}

pub(super) fn validate(state: &Arc<AgentState>) -> ServiceResult<()> {
    crate::session::with_vault(state, false, |_| Ok(()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn polling_does_not_extend_idle_or_absolute_expiry() {
        let sessions = Sessions::default();
        let binding = Binding {
            session_id: Uuid::new_v4(),
            password_revision: [0; 32],
        };
        let (token, csrf) = sessions.issue(binding, 0).unwrap();
        let activity = Instant::now() - Duration::from_secs(60);
        sessions
            .sessions
            .lock()
            .unwrap()
            .get_mut(&digest(token.expose()))
            .unwrap()
            .last_activity = activity;
        sessions
            .authorized(token.expose(), None, || Ok(()))
            .unwrap();
        assert_eq!(
            sessions.sessions.lock().unwrap()[&digest(token.expose())].last_activity,
            activity
        );
        sessions
            .sessions
            .lock()
            .unwrap()
            .get_mut(&digest(token.expose()))
            .unwrap()
            .last_activity = Instant::now() - IDLE;
        assert!(sessions
            .authorized(token.expose(), Some(&csrf), || Ok(()))
            .is_err());
        {
            let mut entries = sessions.sessions.lock().unwrap();
            let entry = entries.get_mut(&digest(token.expose())).unwrap();
            entry.last_activity = Instant::now();
            entry.auth.expires = Instant::now();
        }
        assert!(sessions
            .authorized(token.expose(), Some(&csrf), || Ok(()))
            .is_err());
    }

    #[test]
    fn rotation_rejects_login_that_started_before_revocation() {
        let sessions = Sessions::default();
        let generation = sessions.generation();
        sessions.revoke_all();
        let binding = Binding {
            session_id: Uuid::new_v4(),
            password_revision: [0; 32],
        };
        assert!(sessions.issue(binding, generation).is_err());
        assert_ne!(cookie_name(false, 8788), cookie_name(false, 8789));
    }
}
