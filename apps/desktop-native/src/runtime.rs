//! One exclusion boundary for native Agent requests, startup and replacement.
pub mod agent;
pub mod instance;

use anyhow::{bail, Result};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, RwLock, RwLockReadGuard,
};

pub trait Services: Send + Sync + 'static {
    fn proxy_running(&self) -> Result<bool>;
    fn suspend(&self) -> Result<()>;
    fn stop(&self) -> Result<()>;
    fn restore(&self) -> Result<()>;
}

#[derive(Default)]
pub struct Gate {
    requested: AtomicBool,
    retired: AtomicBool,
    operations: RwLock<()>,
}

impl Gate {
    pub fn enter(&self) -> Result<RwLockReadGuard<'_, ()>> {
        if self.requested.load(Ordering::Acquire) || self.retired.load(Ordering::Acquire) {
            bail!("native runtime is suspended");
        }
        let guard = self
            .operations
            .try_read()
            .map_err(|_| anyhow::anyhow!("native runtime is suspended"))?;
        if self.requested.load(Ordering::Acquire) || self.retired.load(Ordering::Acquire) {
            bail!("native runtime is suspended");
        }
        Ok(guard)
    }
    pub fn suspended(&self) -> bool {
        self.requested.load(Ordering::Acquire) || self.retired.load(Ordering::Acquire)
    }
}

pub struct Runtime {
    gate: Arc<Gate>,
    services: Box<dyn Services>,
}

impl Runtime {
    pub fn new(gate: Arc<Gate>, services: impl Services) -> Self {
        Self {
            gate,
            services: Box::new(services),
        }
    }
    pub fn gate(&self) -> Arc<Gate> {
        self.gate.clone()
    }
    pub fn proxy_running(&self) -> Result<bool> {
        let _guard = self.gate.enter()?;
        self.services.proxy_running()
    }

    /// Return false when fresh proxy state requires confirmation. The operation
    /// closure is called only after all old requests/startups drain and shutdown succeeds.
    pub fn install(
        &self,
        allow_disconnect: bool,
        operation: impl FnOnce() -> Result<()>,
    ) -> Result<bool> {
        self.install_when(allow_disconnect, || true, operation)
    }

    pub fn install_when(
        &self,
        allow_disconnect: bool,
        authorized: impl Fn() -> bool,
        operation: impl FnOnce() -> Result<()>,
    ) -> Result<bool> {
        if self.gate.requested.swap(true, Ordering::AcqRel) {
            bail!("update_busy");
        }
        struct Reset<'a>(&'a AtomicBool);
        impl Drop for Reset<'_> {
            fn drop(&mut self) {
                self.0.store(false, Ordering::Release);
            }
        }
        let _reset = Reset(&self.gate.requested);
        let _guard = self
            .gate
            .operations
            .write()
            .map_err(|_| anyhow::anyhow!("native runtime lock poisoned"))?;
        if self.gate.retired.load(Ordering::Acquire) {
            bail!("native runtime already retired");
        }
        anyhow::ensure!(authorized(), "update confirmation expired");
        if self.services.proxy_running()? && !allow_disconnect {
            return Ok(false);
        }
        aipass_desktop_platform::clipboard::clear_now().map_err(anyhow::Error::msg)?;
        let result = self
            .services
            .suspend()
            .and_then(|()| self.services.stop())
            .and_then(|()| operation());
        match result {
            Ok(()) => {
                // Do not let the old desktop's final status poll restart its Agent.
                self.gate.retired.store(true, Ordering::Release);
                Ok(true)
            }
            Err(error) => {
                if let Err(recovery) = self.services.restore() {
                    self.gate.retired.store(true, Ordering::Release);
                    bail!("{error:#}; runtime recovery failed: {recovery:#}");
                }
                Err(error)
            }
        }
    }
}

#[cfg(test)]
mod tests;
