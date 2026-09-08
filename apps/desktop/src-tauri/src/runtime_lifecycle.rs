//! Coordinate desktop-owned agent startup with bundle replacement.
use std::sync::{RwLock, RwLockReadGuard, RwLockWriteGuard};

pub(crate) static RUNTIME: RuntimeLifecycle = RuntimeLifecycle(RwLock::new(()));

pub(crate) struct RuntimeLifecycle(RwLock<()>);

impl RuntimeLifecycle {
    pub(crate) fn start(&self) -> Result<RwLockReadGuard<'_, ()>, String> {
        // A status poll must never block the native event loop behind an
        // installer that needs that loop to restart the application.
        self.0
            .try_read()
            .map_err(|_| "AIPass runtime is suspended while installing an update".into())
    }

    pub(crate) fn update(&self) -> Result<RwLockWriteGuard<'_, ()>, String> {
        // Finish any startup already in flight before shutting down its agent.
        self.0
            .write()
            .map_err(|_| "AIPass runtime lifecycle lock is poisoned".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{mpsc, Arc};
    use std::time::Duration;

    #[test]
    fn update_waits_for_existing_startup_and_rejects_watchdog_restarts() {
        let lifecycle = Arc::new(RuntimeLifecycle(RwLock::new(())));
        let starting = lifecycle.start().unwrap();
        let (entered, entered_rx) = mpsc::channel();
        let (release, release_rx) = mpsc::channel();
        let worker = lifecycle.clone();
        let updating = std::thread::spawn(move || {
            let _guard = worker.update().unwrap();
            entered.send(()).unwrap();
            release_rx.recv().unwrap();
        });
        assert!(entered_rx.recv_timeout(Duration::from_millis(100)).is_err());
        drop(starting);
        entered_rx.recv_timeout(Duration::from_secs(2)).unwrap();
        assert!(lifecycle.start().is_err());
        release.send(()).unwrap();
        updating.join().unwrap();
        // Failed installs release the guard before restoring normal startup.
        assert!(lifecycle.start().is_ok());
    }
}
