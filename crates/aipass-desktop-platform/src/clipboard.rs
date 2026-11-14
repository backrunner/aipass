//! Shared desktop secret clipboard lifetime. The caller obtains confirmation
//! before reading a secret; this module owns expiry and invalidation.
use aipass_agent_protocol::SensitiveString;
use sha2::{Digest, Sha256};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex, OnceLock};
use std::time::{Duration, Instant};

trait Clipboard {
    fn read(&mut self) -> Result<SensitiveString, String>;
    fn write(&mut self, value: &str) -> Result<(), String>;
    fn clear(&mut self) -> Result<(), String>;
}

#[derive(Default)]
struct NativeClipboard(Option<arboard::Clipboard>);
impl NativeClipboard {
    fn get(&mut self) -> Result<&mut arboard::Clipboard, String> {
        if self.0.is_none() {
            self.0 = Some(arboard::Clipboard::new().map_err(|_| "Clipboard unavailable.")?);
        }
        self.0
            .as_mut()
            .ok_or_else(|| "Clipboard unavailable.".into())
    }
}
impl Clipboard for NativeClipboard {
    fn read(&mut self) -> Result<SensitiveString, String> {
        self.get()?
            .get_text()
            .map(SensitiveString::new)
            .map_err(|_| "Could not read clipboard.".into())
    }
    fn write(&mut self, value: &str) -> Result<(), String> {
        let clipboard = self.get()?;
        #[cfg(target_os = "macos")]
        let result = {
            use arboard::SetExtApple;
            clipboard.set().exclude_from_history().text(value)
        };
        #[cfg(target_os = "windows")]
        let result = {
            use arboard::SetExtWindows;
            clipboard.set().exclude_from_monitoring().text(value)
        };
        #[cfg(not(any(target_os = "macos", target_os = "windows")))]
        let result = clipboard.set_text(value);
        result.map_err(|_| "Could not copy secret.".into())
    }
    fn clear(&mut self) -> Result<(), String> {
        self.get()?
            .clear()
            .map_err(|_| "Could not clear clipboard.".into())
    }
}

struct Pending {
    digest: [u8; 32],
    deadline: Instant,
    retries: u8,
}
#[derive(Default)]
struct ClipboardState<C> {
    clipboard: C,
    pending: Option<Pending>,
    // Lock/exit invalidates a copy still waiting for confirmation or the Agent.
    epoch: u64,
}
impl<C: Clipboard> ClipboardState<C> {
    fn copy(&mut self, epoch: u64, value: SensitiveString, seconds: u16) -> Result<bool, String> {
        if self.epoch != epoch {
            return Ok(false);
        }
        self.clipboard.write(value.expose())?;
        self.pending = Some(Pending {
            digest: Sha256::digest(value.expose().as_bytes()).into(),
            deadline: Instant::now() + Duration::from_secs(u64::from(seconds)),
            retries: 0,
        });
        Ok(true)
    }
    fn clear_matching(&mut self) -> Result<(), String> {
        let Some(pending) = &self.pending else {
            return Ok(());
        };
        // Never clear on a failed read: it could now contain unrelated content.
        let current = self.clipboard.read()?;
        if <[u8; 32]>::from(Sha256::digest(current.expose().as_bytes())) == pending.digest {
            self.clipboard.clear()?;
        }
        self.pending = None;
        Ok(())
    }
    fn clear_or_retry(&mut self) {
        if self.clear_matching().is_err() {
            if let Some(pending) = &mut self.pending {
                // Log once per failed cleanup, then bound work without abandoning ownership.
                if pending.retries == 0 {
                    log::warn!(
                        "Secret clipboard cleanup delayed; clipboard contents were preserved."
                    );
                }
                pending.retries = pending.retries.saturating_add(1);
                let seconds = 1_u64 << pending.retries.saturating_sub(1).min(5);
                pending.deadline = Instant::now() + Duration::from_secs(seconds.min(30));
            }
        }
    }
    fn invalidate(&mut self) -> Result<(), String> {
        self.epoch = self.epoch.wrapping_add(1);
        let result = self.clear_matching();
        if result.is_err() {
            if let Some(pending) = &mut self.pending {
                pending.deadline = Instant::now();
            }
        }
        result
    }
}

type Shared = (Mutex<ClipboardState<NativeClipboard>>, Condvar);
fn shared() -> &'static Shared {
    static SHARED: OnceLock<Shared> = OnceLock::new();
    SHARED.get_or_init(|| (Mutex::new(ClipboardState::default()), Condvar::new()))
}
fn start_worker() {
    static WORKER: OnceLock<()> = OnceLock::new();
    WORKER.get_or_init(|| {
        std::thread::spawn(|| {
            let (mutex, changed) = shared();
            let Ok(mut state) = mutex.lock() else {
                return;
            };
            loop {
                let delay = state
                    .pending
                    .as_ref()
                    .map(|p| p.deadline.saturating_duration_since(Instant::now()));
                state = match delay {
                    None => match changed.wait(state) {
                        Ok(state) => state,
                        Err(_) => return,
                    },
                    Some(delay) if !delay.is_zero() => match changed.wait_timeout(state, delay) {
                        Ok((state, _)) => state,
                        Err(_) => return,
                    },
                    Some(_) => {
                        state.clear_or_retry();
                        state
                    }
                };
            }
        });
    });
}

/// Invalidate in-flight requests immediately and schedule OS cleanup off the UI thread.
pub fn invalidate() {
    start_worker();
    if let Ok(mut state) = shared().0.lock() {
        state.epoch = state.epoch.wrapping_add(1);
        if let Some(pending) = &mut state.pending {
            pending.deadline = Instant::now();
        }
    }
    shared().1.notify_one();
}

pub fn clear_now() -> Result<(), String> {
    let result = shared()
        .0
        .lock()
        .map_err(|_| "Clipboard unavailable.")?
        .invalidate();
    shared().1.notify_one();
    result
}

static COPYING: AtomicBool = AtomicBool::new(false);

/// Cancels a request which outlives its provider selection or confirmation.
#[derive(Clone)]
pub struct Cancellation(Arc<AtomicBool>);
impl Cancellation {
    pub fn cancel(&self) {
        self.0.store(true, Ordering::Release);
    }
}

/// Exclusive, single-use permission captured before presenting confirmation.
/// It carries no secret and cannot be constructed with a caller-chosen epoch.
pub struct CopyPermit {
    epoch: u64,
    cancellation: Cancellation,
}
impl CopyPermit {
    pub fn begin() -> Result<Option<Self>, String> {
        if COPYING
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
        {
            return Ok(None);
        }
        match shared().0.lock() {
            Ok(state) => Ok(Some(Self {
                epoch: state.epoch,
                cancellation: Cancellation(Arc::new(AtomicBool::new(false))),
            })),
            Err(_) => {
                COPYING.store(false, Ordering::Release);
                Err("Clipboard unavailable.".into())
            }
        }
    }
    pub fn cancellation(&self) -> Cancellation {
        self.cancellation.clone()
    }
    pub fn is_current(&self) -> bool {
        !self.cancellation.0.load(Ordering::Acquire)
            && shared()
                .0
                .lock()
                .is_ok_and(|state| state.epoch == self.epoch)
    }
    pub fn write(self, value: SensitiveString, seconds: u16) -> Result<bool, String> {
        start_worker();
        let result = {
            let mut state = shared().0.lock().map_err(|_| "Clipboard unavailable.")?;
            if self.cancellation.0.load(Ordering::Acquire) {
                return Ok(false);
            }
            state.copy(self.epoch, value, normalize_seconds(seconds))
        };
        shared().1.notify_one();
        result
    }
}
impl Drop for CopyPermit {
    fn drop(&mut self) {
        COPYING.store(false, Ordering::Release);
    }
}

/// Both desktop shells use the same concise native confirmation.
pub fn confirmation(
    locale: aipass_agent_protocol::UiLocale,
) -> (&'static str, &'static str, &'static str, &'static str) {
    match locale {
        aipass_agent_protocol::UiLocale::ZhCn => (
            "复制凭据？",
            "剪贴板内容可能被其他应用读取。",
            "复制",
            "取消",
        ),
        aipass_agent_protocol::UiLocale::En => (
            "Copy credential?",
            "Other apps may read clipboard contents.",
            "Copy",
            "Cancel",
        ),
    }
}

pub fn normalize_seconds(seconds: u16) -> u16 {
    if seconds == 0 {
        45
    } else {
        seconds.min(600)
    }
}

#[cfg(test)]
mod tests;
