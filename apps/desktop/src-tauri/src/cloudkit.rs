//! Signed native transport worker. No WebView callbacks, vault access or unlock.
use aipass_agent_protocol::{AgentRequest, CloudKitCompletion, CloudKitReply, CloudKitTask};
use std::ffi::{c_char, CStr, CString};
use std::sync::atomic::{AtomicBool, Ordering};
static CHANGED: AtomicBool = AtomicBool::new(false);
extern "C" {
    fn aipass_cloudkit_execute(input: *const c_char) -> *mut c_char;
    fn aipass_cloudkit_free(pointer: *mut c_char);
    fn aipass_cloudkit_observe(callback: extern "C" fn());
}
extern "C" fn changed() {
    CHANGED.store(true, Ordering::Relaxed);
}

pub fn start(app: tauri::AppHandle) {
    unsafe {
        aipass_cloudkit_observe(changed);
    }
    std::thread::spawn(move || {
        let mut completion = None;
        while !crate::ALLOW_PROCESS_EXIT.load(Ordering::Relaxed) {
            let changed = CHANGED.swap(false, Ordering::Relaxed);
            let response = crate::agent_client(&app).and_then(|client| {
                client
                    .request_raw(&AgentRequest::CloudKitExchange {
                        completion: completion.clone(),
                        changed,
                    })
                    .map_err(|err| err.to_string())
            });
            let task = response
                .ok()
                .filter(|response| response.ok)
                .and_then(|response| {
                    serde_json::from_value::<Option<CloudKitTask>>(response.data).ok()
                });
            match task {
                Some(Some(task)) => {
                    completion = Some(CloudKitCompletion {
                        id: task.id,
                        reply: execute(&task),
                    })
                }
                Some(None) => completion = None,
                None => {
                    if changed {
                        CHANGED.store(true, Ordering::Relaxed);
                    }
                    std::thread::sleep(std::time::Duration::from_secs(1));
                }
            }
        }
    });
}

fn execute(task: &CloudKitTask) -> CloudKitReply {
    let error = || CloudKitReply {
        error: Some("CloudKit native bridge failed".into()),
        ..Default::default()
    };
    let Some(input) = serde_json::to_string(task)
        .ok()
        .and_then(|text| CString::new(text).ok())
    else {
        return error();
    };
    unsafe {
        let pointer = aipass_cloudkit_execute(input.as_ptr());
        if pointer.is_null() {
            return error();
        }
        let result = serde_json::from_slice(CStr::from_ptr(pointer).to_bytes());
        aipass_cloudkit_free(pointer);
        result.unwrap_or_else(|_| error())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn unsigned_native_cloudkit_boundary_fails_without_accessing_an_apple_account() {
        let started = std::time::Instant::now();
        let task = CloudKitTask {
            id: uuid::Uuid::new_v4(),
            command: aipass_agent_protocol::CloudKitCommand::List,
            account: None,
        };
        for _ in 0..3 {
            let reply = execute(&task);
            assert!(matches!(
                reply.error_kind,
                Some(aipass_agent_protocol::CloudKitErrorKind::Unavailable)
            ));
            assert!(reply.error.unwrap().contains("signed AIPass app"));
            assert!(reply.account.is_none());
        }
        assert!(started.elapsed() < std::time::Duration::from_secs(3));
    }
}
