use std::path::Path;

pub fn native_manifest(host_path: &Path, allowed_origins: &[String]) -> serde_json::Value {
    serde_json::json!({
        "name": "dev.aipass.native",
        "description": "AIPass native messaging host",
        "path": host_path,
        "type": "stdio",
        "allowed_origins": allowed_origins,
    })
}
