use super::*;
pub(super) enum Account {
    Claude(crate::claude_cli::NativeAccount),
    Auth(SensitiveString),
}
pub(super) type ReadResult =
    Result<Account, (SubscriptionImportStatus, &'static str, &'static str)>;

pub(super) async fn read(
    source: &SubscriptionImportSource,
    outbound: &aipass_proxy::UpstreamProxyConfig,
) -> ReadResult {
    // Isolate filesystem/database and CLI calls from the timer/cancellation
    // executor. A stalled store cannot occupy all four async workers.
    let source = source.clone();
    let outbound = outbound.clone();
    let cancelled = Arc::new(AtomicBool::new(false));
    let _cancel_on_drop = CancelRead(cancelled.clone());
    tokio::task::spawn_blocking(move || {
        // Keep this reader's reactor alive independently of the task runtime.
        // Cancellation must drop an HTTP future, rather than shut down the
        // reactor underneath its timeout and trigger a Tokio panic.
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|_| (SubscriptionImportStatus::Failed, "read_failed", "retry"))?;
        let result = runtime.block_on(async {
            let read = read_source(&source, &outbound);
            tokio::pin!(read);
            loop {
                if cancelled.load(Ordering::Acquire) {
                    break Err((SubscriptionImportStatus::Cancelled, "cancelled", "retry"));
                }
                tokio::select! {
                    result = &mut read => break result,
                    _ = tokio::time::sleep(Duration::from_millis(50)) => {},
                }
            }
        });
        runtime.shutdown_timeout(Duration::from_millis(100));
        result
    })
    .await
    .unwrap_or(Err((
        SubscriptionImportStatus::Failed,
        "read_failed",
        "retry",
    )))
}
struct CancelRead(Arc<AtomicBool>);
impl Drop for CancelRead {
    fn drop(&mut self) {
        self.0.store(true, Ordering::Release);
    }
}
async fn read_source(
    source: &SubscriptionImportSource,
    outbound: &aipass_proxy::UpstreamProxyConfig,
) -> ReadResult {
    let normalized = sources::normalize(source.clone()).map_err(|_| {
        (
            SubscriptionImportStatus::Failed,
            "invalid_source",
            "choose_directory",
        )
    })?;
    let source = &normalized;
    if sources::native_cli(&source.provider) {
        if source.provider == "anthropic" {
            return crate::claude_cli::local_account(&source.root)
                .map(Account::Claude)
                .map_err(|e| classify(&e));
        }
        let status = crate::subscriptions::cli_accounts::status(&source.provider);
        if !status.available {
            return Err((
                SubscriptionImportStatus::NotFound,
                "cli_unavailable",
                "install_cli",
            ));
        }
        let auth =
            crate::subscriptions::cli_accounts::import_reference(&source.provider, &source.root)
                .map_err(|e| classify(&e))?;
        return Ok(Account::Auth(SensitiveString::new(auth.to_string())));
    }
    if let Some(file) = crate::subscriptions::native_import::required_file(source) {
        match std::fs::metadata(&file) {
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                return Err((SubscriptionImportStatus::NotFound, "not_found", "login"))
            }
            Err(_) => {
                return Err((
                    SubscriptionImportStatus::Failed,
                    "permission_denied",
                    "retry",
                ))
            }
            Ok(meta) if !meta.is_file() || meta.len() > 8 * 1024 * 1024 => {
                return Err((
                    SubscriptionImportStatus::Failed,
                    "invalid_format",
                    "choose_directory",
                ))
            }
            _ => {}
        }
        if let Err(e) = std::fs::File::open(&file) {
            return Err(if e.kind() == std::io::ErrorKind::PermissionDenied {
                (
                    SubscriptionImportStatus::Failed,
                    "permission_denied",
                    "retry",
                )
            } else {
                (SubscriptionImportStatus::Failed, "read_failed", "retry")
            });
        }
    }
    crate::subscriptions::native_import::read(source, outbound)
        .await
        .map(Account::Auth)
        .map_err(|e| classify(&e))
}
fn classify(error: &str) -> (SubscriptionImportStatus, &'static str, &'static str) {
    let e = error.to_ascii_lowercase();
    if e.contains("permission denied") {
        (
            SubscriptionImportStatus::Failed,
            "permission_denied",
            "retry",
        )
    } else if e.contains("not installed") {
        (
            SubscriptionImportStatus::NotFound,
            "cli_unavailable",
            "install_cli",
        )
    } else if e.contains("secure store") || e.contains("keychain") || e.contains("credential store")
    {
        (
            SubscriptionImportStatus::Failed,
            "secure_store_unavailable",
            "retry",
        )
    } else if e.contains("not found") {
        (SubscriptionImportStatus::NotFound, "not_found", "login")
    } else if e.contains("expired")
        || e.contains("not signed in")
        || e.contains("no account")
        || e.contains("identity")
        || e.contains("no access token")
        || e.contains("sign in")
    {
        (SubscriptionImportStatus::NeedsLogin, "needs_login", "login")
    } else if e.contains("invalid") || e.contains("exceed") {
        (
            SubscriptionImportStatus::Failed,
            "invalid_format",
            "choose_directory",
        )
    } else {
        (SubscriptionImportStatus::Failed, "read_failed", "retry")
    }
}

pub(crate) struct PrivateAuth(pub Value);
impl Drop for PrivateAuth {
    fn drop(&mut self) {
        crate::subscriptions::native_import::erase(&mut self.0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn vendor_failures_keep_normal_absence_distinct_from_login_and_permissions() {
        for (message, status, code) in [
            (
                "Claude Code did not sign in to a Claude subscription",
                SubscriptionImportStatus::NeedsLogin,
                "needs_login",
            ),
            (
                "native sign-in not found; sign in first",
                SubscriptionImportStatus::NotFound,
                "not_found",
            ),
            (
                "native sign-in permission denied",
                SubscriptionImportStatus::Failed,
                "permission_denied",
            ),
            (
                "Copilot CLI credential store is unavailable",
                SubscriptionImportStatus::Failed,
                "secure_store_unavailable",
            ),
            (
                "invalid native sign-in fixture-private-token",
                SubscriptionImportStatus::Failed,
                "invalid_format",
            ),
        ] {
            let error = classify(message);
            assert_eq!(error.0, status);
            assert_eq!(error.1, code);
            assert!(!error.1.contains("fixture-private"));
        }
    }
}
