use super::*;

const RETAINED_EVENTS: i64 = 10_000;

impl UsageStore {
    pub(super) fn init_diagnostics(connection: &Connection) -> Result<(), ProxyError> {
        connection
            .execute_batch(
                "CREATE TABLE IF NOT EXISTS proxy_diagnostics (
                sequence INTEGER PRIMARY KEY AUTOINCREMENT,
                timestamp INTEGER NOT NULL, level TEXT NOT NULL, message TEXT NOT NULL
            );",
            )
            .map_err(ProxyError::Sqlite)
    }

    /// Callers supply only fixed event names, UUIDs, status codes and numbers.
    /// URLs, models, credential labels and arbitrary upstream errors stay out.
    pub(super) fn log_diagnostic(&self, level: &'static str, message: String) {
        if self.append_diagnostic(level, &message).is_err() {
            eprintln!("AIPass: failed to persist proxy diagnostics");
        }
    }

    fn append_diagnostic(&self, level: &str, message: &str) -> Result<(), ProxyError> {
        let mut connection = self.connection.lock().map_err(|_| ProxyError::Poisoned)?;
        let tx = connection.transaction().map_err(ProxyError::Sqlite)?;
        tx.execute(
            "INSERT INTO proxy_diagnostics(timestamp, level, message) VALUES (?1, ?2, ?3)",
            params![now_unix(), level, message],
        )
        .map_err(ProxyError::Sqlite)?;
        tx.execute(
            "DELETE FROM proxy_diagnostics WHERE sequence <= (SELECT MAX(sequence) FROM proxy_diagnostics) - ?1",
            [RETAINED_EVENTS],
        ).map_err(ProxyError::Sqlite)?;
        tx.commit().map_err(ProxyError::Sqlite)
    }

    pub fn logs(&self) -> Result<Vec<ProxyLogEntry>, ProxyError> {
        let connection = self.connection.lock().map_err(|_| ProxyError::Poisoned)?;
        let mut statement = connection.prepare(
            "SELECT timestamp, level, message FROM (SELECT sequence, timestamp, level, message FROM proxy_diagnostics ORDER BY sequence DESC LIMIT ?1) ORDER BY sequence",
        ).map_err(ProxyError::Sqlite)?;
        let logs = statement
            .query_map([MAX_PROXY_LOG_ENTRIES as i64], |row| {
                Ok(ProxyLogEntry {
                    timestamp: row.get(0)?,
                    level: row.get(1)?,
                    message: row.get(2)?,
                })
            })
            .map_err(ProxyError::Sqlite)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(ProxyError::Sqlite)?;
        Ok(logs)
    }

    pub(super) fn log_request(&self, item: &UsageRecord) {
        self.log_diagnostic(if item.status < 400 { "info" } else { "error" }, format!(
            "event=proxy.request.completed request_id={} route_id={} provider_id={} status={} attempts={} duration_ms={} first_token_ms={:?}",
            item.id, item.route_id, item.provider_entry_id, item.status, item.attempts, item.duration_ms, item.first_token_ms,
        ));
    }

    pub(super) fn log_attempt(&self, item: &AttemptRecord) {
        self.log_diagnostic(if item.success == Some(false) { "warn" } else { "info" }, format!(
            "event=proxy.attempt.completed request_id={} attempt_id={} route_id={} target_id={} provider_id={} status={:?} outcome={} duration_ms={}",
            item.request_id.map(|id| id.to_string()).unwrap_or_else(|| "legacy".into()),
            item.id, item.route_id, item.target_id, item.provider_entry_id, item.status,
            match item.success { Some(true) => "success", Some(false) => "failed", None => "client_disconnected" },
            item.duration_ms,
        ));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn diagnostics_survive_restart_and_usage_clear_and_are_bounded() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("usage.sqlite");
        let store = UsageStore::open(&path).unwrap();
        store
            .append_diagnostic("info", "event=proxy.started")
            .unwrap();
        // Populate enough history in one transaction to exercise actual pruning.
        store
            .connection
            .lock()
            .unwrap()
            .execute_batch(
                "WITH RECURSIVE n(x) AS (SELECT 1 UNION ALL SELECT x+1 FROM n WHERE x<10000)
             INSERT INTO proxy_diagnostics(timestamp,level,message) SELECT 1,'info','old' FROM n;",
            )
            .unwrap();
        store
            .append_diagnostic("warn", "event=proxy.test.latest")
            .unwrap();
        store.clear().unwrap();
        drop(store);
        let reopened = UsageStore::open(path).unwrap();
        assert_eq!(
            reopened.logs().unwrap().last().unwrap().message,
            "event=proxy.test.latest"
        );
        let count: i64 = reopened
            .connection
            .lock()
            .unwrap()
            .query_row("SELECT COUNT(*) FROM proxy_diagnostics", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(count, RETAINED_EVENTS);
    }
}
