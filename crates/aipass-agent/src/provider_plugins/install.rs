//! Verification runs outside the registry lock. Only a completed check can persist a grant.
use super::*;
use tokio::sync::watch;

pub(super) struct Check {
    pub id: String,
    pub cancel: watch::Sender<bool>,
}

pub(crate) struct Installation {
    root: PathBuf,
    grant: Grant,
    package: Option<(uuid::Uuid, tempfile::TempDir)>,
    launch: Launch,
    check: Arc<Check>,
}
pub(crate) struct Verified(Installation);

impl Registry {
    pub fn prepare_install(
        &mut self,
        vault: &Path,
        action: SubscriptionPluginAction,
    ) -> Result<Installation> {
        let root = root(vault)?;
        if self.checking.as_ref().and_then(Weak::upgrade).is_some() {
            return Err("Plugin verification already in progress".into());
        }
        let (grant, launch, package) = match action {
            SubscriptionPluginAction::Enable { path, digest } => {
                let (grant, program) = inspect(&path)?;
                if grant.digest != digest {
                    return Err("Plugin changed; inspect it again".into());
                }
                let launch = stage(&grant, &program)?;
                (grant, launch, None)
            }
            SubscriptionPluginAction::EnableGithub { review, digest } => {
                let pending = self
                    .pending
                    .as_ref()
                    .filter(|p| {
                        p.token == review
                            && p.digest == digest
                            && p.created.elapsed() < std::time::Duration::from_secs(300)
                    })
                    .ok_or("Plugin review expired")?;
                let packages = root.join("packages");
                create_registry_dir(&packages)?;
                let dir = private_stage(Some(&packages))?;
                let path = dir.path().join("manifest.json");
                fs::write(&path, &pending.raw)
                    .and_then(|()| {
                        fs::write(dir.path().join(&pending.manifest.entry), &pending.module)
                    })
                    .map_err(|_| "Cannot save GitHub plugin")?;
                let grant = Grant {
                    path,
                    digest,
                    manifest: pending.manifest.clone(),
                    origin: Some(pending.origin.clone()),
                };
                let launch = stage(&grant, &pending.module)?;
                (grant, launch, Some((review, dir)))
            }
            _ => return Err("Expected a plugin install request".into()),
        };
        if root
            .join(format!("{}.json", grant.manifest.id))
            .try_exists()
            .map_err(|_| "Plugin registry unavailable")?
        {
            return Err("Disable the existing plugin before enabling a replacement".into());
        }
        let check = Arc::new(Check {
            id: grant.manifest.id.clone(),
            cancel: watch::channel(false).0,
        });
        self.checking = Some(Arc::downgrade(&check));
        Ok(Installation {
            root,
            grant,
            package,
            launch,
            check,
        })
    }
}

impl Installation {
    pub fn verify(self) -> Result<Verified> {
        super::super::subscriptions::verify_plugin(&self.launch, self.check.cancel.subscribe())?;
        Ok(Verified(self))
    }
}

impl Registry {
    /// The caller holds the registry lock, serializing revocation with the durable write.
    pub fn commit_install(&mut self, verified: Verified) -> Result<Value> {
        let install = verified.0;
        if self
            .checking
            .as_ref()
            .is_none_or(|active| !active.ptr_eq(&Arc::downgrade(&install.check)))
            || *install.check.cancel.borrow()
        {
            return Err("Plugin operation revoked".into());
        }
        let (current, _) = read_package(&install.grant.path)?;
        if current.digest != install.grant.digest {
            return Err("Plugin changed; inspect it again".into());
        }
        save_grant(&install.root, &install.grant)?;
        if let Some((review, dir)) = install.package {
            let _ = dir.keep();
            if self.pending.as_ref().is_some_and(|p| p.token == review) {
                self.pending = None;
            }
        }
        Ok(install.grant.summary())
    }
}
