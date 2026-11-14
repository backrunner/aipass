use super::*;
use std::sync::{mpsc, Mutex};
use std::time::Duration;

#[derive(Default)]
struct Fake {
    events: Mutex<Vec<&'static str>>,
    proxy: AtomicBool,
    stop_error: AtomicBool,
    recovery_error: AtomicBool,
}
impl Services for Arc<Fake> {
    fn proxy_running(&self) -> Result<bool> {
        Ok(self.proxy.load(Ordering::Relaxed))
    }
    fn suspend(&self) -> Result<()> {
        self.events.lock().unwrap().push("suspend");
        Ok(())
    }
    fn stop(&self) -> Result<()> {
        self.events.lock().unwrap().push("stop");
        if self.stop_error.load(Ordering::Relaxed) {
            bail!("stop failed");
        }
        Ok(())
    }
    fn restore(&self) -> Result<()> {
        self.events.lock().unwrap().push("restore");
        if self.recovery_error.load(Ordering::Relaxed) {
            bail!("restore failed");
        }
        Ok(())
    }
}

#[test]
fn update_drains_requests_rejects_new_startups_and_retires_old_runtime() {
    let gate = Arc::new(Gate::default());
    let fake = Arc::new(Fake::default());
    let runtime = Runtime::new(gate.clone(), fake.clone());
    let read = gate.enter().unwrap();
    let (started, rx) = mpsc::channel();
    let worker = std::thread::spawn(move || {
        runtime.install(false, || {
            started.send(()).unwrap();
            Ok(())
        })
    });
    assert!(rx.recv_timeout(Duration::from_millis(50)).is_err());
    assert!(gate.enter().is_err());
    drop(read);
    rx.recv_timeout(Duration::from_secs(2)).unwrap();
    assert!(worker.join().unwrap().unwrap());
    assert!(gate.enter().is_err());
    assert_eq!(*fake.events.lock().unwrap(), ["suspend", "stop"]);
}

#[test]
fn fresh_proxy_state_requires_confirmation_without_stopping_anything() {
    let fake = Arc::new(Fake::default());
    fake.proxy.store(true, Ordering::Relaxed);
    let runtime = Runtime::new(Arc::new(Gate::default()), fake.clone());
    assert!(!runtime
        .install(false, || panic!("unconfirmed installation"))
        .unwrap());
    assert!(fake.events.lock().unwrap().is_empty());
    assert!(runtime.gate.enter().is_ok());
    assert!(runtime.install(true, || Ok(())).unwrap());
}

#[test]
fn shutdown_and_install_failures_restore_before_reopening_requests() {
    for stop_error in [false, true] {
        let fake = Arc::new(Fake::default());
        fake.stop_error.store(stop_error, Ordering::Relaxed);
        let runtime = Runtime::new(Arc::new(Gate::default()), fake.clone());
        assert!(runtime.install(false, || bail!("install failed")).is_err());
        assert_eq!(*fake.events.lock().unwrap(), ["suspend", "stop", "restore"]);
        assert!(runtime.gate.enter().is_ok());
    }
}

#[test]
fn failed_recovery_stays_closed_instead_of_restarting_from_polling() {
    let fake = Arc::new(Fake::default());
    fake.recovery_error.store(true, Ordering::Relaxed);
    let runtime = Runtime::new(Arc::new(Gate::default()), fake);
    assert!(runtime.install(false, || bail!("failed install")).is_err());
    assert!(runtime.gate.enter().is_err());
}

#[test]
fn locking_while_requests_drain_invalidates_pending_consent() {
    let gate = Arc::new(Gate::default());
    let fake = Arc::new(Fake::default());
    let runtime = Runtime::new(gate.clone(), fake.clone());
    let authorized = Arc::new(AtomicBool::new(true));
    let worker_authorized = authorized.clone();
    let read = gate.enter().unwrap();
    let worker = std::thread::spawn(move || {
        runtime.install_when(
            true,
            || worker_authorized.load(Ordering::Acquire),
            || panic!("expired consent must not replace the application"),
        )
    });
    let deadline = std::time::Instant::now() + Duration::from_secs(2);
    while !gate.suspended() {
        assert!(std::time::Instant::now() < deadline);
        std::thread::yield_now();
    }
    authorized.store(false, Ordering::Release);
    drop(read);
    assert!(worker.join().unwrap().is_err());
    assert!(fake.events.lock().unwrap().is_empty());
    assert!(gate.enter().is_ok());
}
