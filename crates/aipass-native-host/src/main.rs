use aipass_agent::logging::{
    init_component_logging, install_panic_logger, write_component_log, NATIVE_HOST_LOG,
};
use aipass_native_host::{
    handle_request, read_message, write_message, NativeRequest, NativeResponse,
};
use anyhow::{Context, Result};
use std::io::{stdin, stdout, ErrorKind, Write};

fn main() -> Result<()> {
    let log_path = init_component_logging(NATIVE_HOST_LOG).ok();
    install_panic_logger(NATIVE_HOST_LOG);
    write_component_log(
        NATIVE_HOST_LOG,
        "INFO",
        &format!(
            "native host process starting pid={} log_path={}",
            std::process::id(),
            log_path
                .as_ref()
                .map(|path| path.display().to_string())
                .unwrap_or_else(|| "unavailable".to_string())
        ),
    );
    let stdin = stdin();
    let stdout = stdout();
    let mut reader = stdin.lock();
    let mut writer = stdout.lock();
    loop {
        let request = match read_message(&mut reader) {
            Ok(request) => request,
            Err(err)
                if err
                    .downcast_ref::<std::io::Error>()
                    .is_some_and(|io| io.kind() == ErrorKind::UnexpectedEof) =>
            {
                write_component_log(NATIVE_HOST_LOG, "INFO", "stdin reached EOF; exiting");
                break;
            }
            Err(err) => {
                write_component_log(
                    NATIVE_HOST_LOG,
                    "ERROR",
                    &format!("failed to read native message: {err}"),
                );
                return Err(err).context("failed to read native message");
            }
        };
        write_component_log(
            NATIVE_HOST_LOG,
            "INFO",
            &format!(
                "received request id={} type={}",
                native_request_id(&request),
                native_request_type(&request)
            ),
        );
        let response = handle_request(request);
        log_native_response(&response);
        if let Err(err) = write_message(&mut writer, &response) {
            write_component_log(
                NATIVE_HOST_LOG,
                "ERROR",
                &format!("failed to write native response id={}: {err}", response.id),
            );
            return Err(err);
        }
        if let Err(err) = writer.flush() {
            write_component_log(
                NATIVE_HOST_LOG,
                "ERROR",
                &format!("failed to flush native response id={}: {err}", response.id),
            );
            return Err(err.into());
        }
    }
    Ok(())
}

fn log_native_response(response: &NativeResponse) {
    let status = if response.ok { "ok" } else { "error" };
    let error = response.error.as_deref().unwrap_or("");
    write_component_log(
        NATIVE_HOST_LOG,
        if response.ok { "INFO" } else { "ERROR" },
        &format!(
            "responding id={} status={status} error={error}",
            response.id
        ),
    );
}

fn native_request_id(request: &NativeRequest) -> uuid::Uuid {
    match request {
        NativeRequest::Ping { id, .. }
        | NativeRequest::ContextLookup { id, .. }
        | NativeRequest::EntriesList { id, .. }
        | NativeRequest::EntriesSearch { id, .. }
        | NativeRequest::IsOriginIgnored { id, .. }
        | NativeRequest::IgnoreOrigin { id, .. }
        | NativeRequest::SecretFill { id, .. }
        | NativeRequest::SaveDetected { id, .. }
        | NativeRequest::PreviewDetected { id, .. }
        | NativeRequest::ProviderAdd { id, .. }
        | NativeRequest::ProviderUpdate { id, .. }
        | NativeRequest::ProviderFaviconBackfill { id, .. }
        | NativeRequest::ProviderDelete { id, .. }
        | NativeRequest::UnlockRequest { id, .. }
        | NativeRequest::SessionUnlock { id, .. }
        | NativeRequest::UiOpenMain { id, .. } => *id,
    }
}

fn native_request_type(request: &NativeRequest) -> &'static str {
    match request {
        NativeRequest::Ping { .. } => "ping",
        NativeRequest::ContextLookup { .. } => "context.lookup",
        NativeRequest::EntriesList { .. } => "entries.list",
        NativeRequest::EntriesSearch { .. } => "entries.search",
        NativeRequest::IsOriginIgnored { .. } => "settings.isOriginIgnored",
        NativeRequest::IgnoreOrigin { .. } => "settings.ignoreOrigin",
        NativeRequest::SecretFill { .. } => "secret.fill",
        NativeRequest::SaveDetected { .. } => "secret.saveDetected",
        NativeRequest::PreviewDetected { .. } => "secret.previewDetected",
        NativeRequest::ProviderAdd { .. } => "provider.add",
        NativeRequest::ProviderUpdate { .. } => "provider.update",
        NativeRequest::ProviderFaviconBackfill { .. } => "provider.faviconBackfill",
        NativeRequest::ProviderDelete { .. } => "provider.delete",
        NativeRequest::UnlockRequest { .. } => "unlock.request",
        NativeRequest::SessionUnlock { .. } => "session.unlock",
        NativeRequest::UiOpenMain { .. } => "ui.open_main",
    }
}
