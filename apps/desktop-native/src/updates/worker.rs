use super::{Release, Updater, INTERVAL};
use anyhow::{Context, Result};
use std::{
    fs,
    sync::{
        atomic::{AtomicU64, Ordering},
        mpsc, Arc, Mutex,
    },
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

#[derive(Clone, Default, Debug, serde::Serialize)]
pub struct Snapshot {
    pub busy: bool,
    pub downloaded: u64,
    pub available: Option<Release>,
    pub error: Option<String>,
    pub restarting: bool,
    pub confirmation: Option<u64>,
}

pub enum Command {
    Check,
    Install,
    Confirm(u64),
    Dismiss,
    Stop,
}

#[derive(Default)]
pub struct Host {
    pub runtime: Option<Arc<crate::runtime::Runtime>>,
    pub instance: Option<crate::runtime::instance::Instance>,
}

pub struct Worker {
    sender: mpsc::SyncSender<(Command, u64)>,
    epoch: Arc<AtomicU64>,
    state: Arc<Mutex<Snapshot>>,
}

impl Worker {
    pub fn start(updater: Updater, args: Vec<String>) -> Result<Self> {
        Self::with_host(updater, args, Host::default())
    }

    pub fn with_host(updater: Updater, args: Vec<String>, host: Host) -> Result<Self> {
        let (sender, receiver) = mpsc::sync_channel(1);
        let epoch = Arc::new(AtomicU64::new(1));
        let worker_epoch = epoch.clone();
        let state = Arc::new(Mutex::new(Snapshot::default()));
        let shared = state.clone();
        std::thread::Builder::new()
            .name("native-updater".into())
            .spawn(move || {
                let pending = updater.has_pending();
                let mut pending_install = pending;
                let mut due = Instant::now()
                    + if pending {
                        Duration::ZERO
                    } else {
                        updater.initial_delay(now())
                    };
                loop {
                    let (command, generation) = match receiver
                        .recv_timeout(due.saturating_duration_since(Instant::now()))
                    {
                        Ok((Command::Stop, _)) | Err(mpsc::RecvTimeoutError::Disconnected) => break,
                        Ok(command) => command,
                        Err(mpsc::RecvTimeoutError::Timeout) => {
                            (Command::Check, worker_epoch.load(Ordering::Acquire))
                        }
                    };
                    shared.lock().unwrap().busy = true;
                    shared.lock().unwrap().error = None;
                    let operation = Operation {
                        updater: &updater,
                        shared: &shared,
                        args: &args,
                        host: &host,
                        epoch: &worker_epoch,
                        generation,
                    };
                    let result = operation.run(command, &mut pending_install);
                    let mut state = shared.lock().unwrap();
                    state.busy = false;
                    if let Err(error) = result {
                        state.error = Some(format!("{error:#}"));
                    }
                    // Failed checks retry next launch, or after one day while open. No hot loop.
                    due = Instant::now() + INTERVAL;
                    #[cfg(feature = "parity-render")]
                    if updater.allow_loopback {
                        let _ = aipass_storage::atomic_write_bytes(
                            updater.config.cache_dir.join("status.json"),
                            &serde_json::to_vec(&*state).unwrap(),
                        );
                    }
                    if state.restarting {
                        break;
                    }
                }
            })?;
        Ok(Self {
            sender,
            state,
            epoch,
        })
    }

    pub fn send(&self, command: Command) -> bool {
        if self.state.lock().unwrap().busy {
            return false;
        }
        self.sender
            .try_send((command, self.epoch.load(Ordering::Acquire)))
            .is_ok()
    }
    pub fn cancel_confirmation(&self) {
        self.epoch.fetch_add(1, Ordering::AcqRel);
        self.state.lock().unwrap().confirmation = None;
    }
    pub fn snapshot(&self) -> Snapshot {
        self.state.lock().unwrap().clone()
    }
}

impl Drop for Worker {
    fn drop(&mut self) {
        let _ = self.sender.try_send((Command::Stop, 0));
    }
}

struct Operation<'a> {
    updater: &'a Updater,
    shared: &'a Mutex<Snapshot>,
    args: &'a [String],
    host: &'a Host,
    epoch: &'a AtomicU64,
    generation: u64,
}

impl Operation<'_> {
    fn valid(&self) -> bool {
        self.epoch.load(Ordering::Acquire) == self.generation
    }

    fn run(&self, command: Command, pending: &mut bool) -> Result<()> {
        let updater = self.updater;
        fs::create_dir_all(&updater.config.cache_dir)?;
        let lock = fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .open(updater.config.cache_dir.join("operation.lock"))?;
        lock.lock()?;
        match command {
            Command::Check => {
                let install_pending = std::mem::take(pending);
                let Some(release) = updater.check()? else {
                    updater.clear()?;
                    self.shared.lock().unwrap().available = None;
                    updater.checked(now())?;
                    return Ok(());
                };
                let valid_pending = install_pending && updater.cached(&release)?.is_some();
                updater.download(&release, |bytes| {
                    self.shared.lock().unwrap().downloaded = bytes
                })?;
                updater.checked(now())?;
                if valid_pending {
                    self.shared.lock().unwrap().available = Some(release.clone());
                    self.install(&release, false)?;
                } else if updater.schedule().dismissed.as_ref() != Some(&release.version) {
                    self.shared.lock().unwrap().available = Some(release);
                }
            }
            Command::Install | Command::Confirm(_) => {
                if !self.valid() {
                    return Ok(());
                }
                let consent = match command {
                    Command::Confirm(token) => {
                        let mut state = self.shared.lock().unwrap();
                        if token != self.generation || state.confirmation != Some(token) {
                            return Ok(());
                        }
                        state.confirmation = None;
                        true
                    }
                    _ => false,
                };
                let release = self
                    .shared
                    .lock()
                    .unwrap()
                    .available
                    .clone()
                    .context("no pending update")?;
                self.install(&release, consent)?;
            }
            Command::Dismiss => {
                self.shared.lock().unwrap().confirmation = None;
                if let Some(release) = self.shared.lock().unwrap().available.take() {
                    updater.dismiss(release.version)?;
                }
            }
            Command::Stop => {}
        }
        Ok(())
    }

    fn install(&self, release: &Release, consent: bool) -> Result<()> {
        let prepared = self.updater.prepare_install(release)?;
        if !self.valid() {
            return Ok(());
        }
        let perform = || {
            anyhow::ensure!(self.valid(), "update confirmation expired");
            self.updater.clear()?;
            prepared.restart(self.args, self.host.instance.as_ref())
        };
        let installed = if let Some(runtime) = &self.host.runtime {
            runtime.install_when(consent, || self.valid(), perform)?
        } else {
            perform()?;
            true
        };
        let mut state = self.shared.lock().unwrap();
        if installed {
            state.restarting = true;
        } else if self.valid() {
            state.confirmation = Some(self.generation);
        }
        Ok(())
    }
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lock_invalidates_an_install_already_waiting_in_the_queue() {
        let directory = tempfile::tempdir().unwrap();
        let (updater, _) = crate::updates::tests::updater(directory.path());
        let (sender, receiver) = mpsc::sync_channel(1);
        let worker = Worker {
            sender,
            epoch: Arc::new(AtomicU64::new(7)),
            state: Arc::new(Mutex::new(Snapshot {
                confirmation: Some(7),
                ..Default::default()
            })),
        };
        assert!(worker.send(Command::Confirm(7)));
        worker.cancel_confirmation();
        let (command, generation) = receiver.recv().unwrap();
        assert_eq!(worker.snapshot().confirmation, None);
        let host = Host::default();
        let operation = Operation {
            updater: &updater,
            shared: &worker.state,
            args: &[],
            host: &host,
            epoch: &worker.epoch,
            generation,
        };
        // There is deliberately no available release: stale consent must return
        // before resolving a package, making a network request or installing.
        operation.run(command, &mut false).unwrap();
        operation.run(Command::Install, &mut false).unwrap();
        assert!(!worker.snapshot().restarting);
    }

    #[test]
    fn confirmation_token_cannot_be_replayed_after_dismissal() {
        let directory = tempfile::tempdir().unwrap();
        let (updater, _) = crate::updates::tests::updater(directory.path());
        let state = Mutex::new(Snapshot::default());
        let host = Host::default();
        let epoch = AtomicU64::new(4);
        let operation = Operation {
            updater: &updater,
            shared: &state,
            args: &[],
            host: &host,
            epoch: &epoch,
            generation: 4,
        };
        operation.run(Command::Confirm(3), &mut false).unwrap();
        operation.run(Command::Confirm(4), &mut false).unwrap();
        assert!(!state.lock().unwrap().restarting);
    }
}
