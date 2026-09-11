use super::*;

pub struct ProxyHandle {
    pub(crate) state: RuntimeState,
    pub(crate) stop: Option<oneshot::Sender<()>>,
    pub(crate) thread: Option<std::thread::JoinHandle<()>>,
    pub(crate) bind_addr: String,
}

impl ProxyHandle {
    pub fn start(config: RuntimeConfig, usage: Arc<UsageStore>) -> Result<Self, ProxyError> {
        for route in &config.routes {
            for target in route.targets.iter().filter(|target| target.config.enabled) {
                let target_protocol = target
                    .config
                    .effective_protocol(route.config.upstream_protocol);
                let inbound = route.config.inbound_protocol;
                if inbound != target_protocol && !route.config.conversion_enabled {
                    return Err(ProxyError::InvalidConfig(format!(
                        "route {} target {} speaks {target_protocol:?} but the route accepts {inbound:?}; enable protocol conversion for this route",
                        route.config.name, target.config.label
                    )));
                }
                if !aipass_proxy_conversion::supports(inbound, target_protocol) {
                    return Err(ProxyError::InvalidConfig(format!(
                        "route {} target {}: protocol conversion {inbound:?} -> {target_protocol:?} is not supported",
                        route.config.name, target.config.label
                    )));
                }
            }
        }
        let bind_addr = config.bind_addr.clone();
        let socket: SocketAddr = bind_addr
            .parse()
            .map_err(|_| ProxyError::InvalidConfig("bind address must be host:port".into()))?;
        let ws_health = websocket::capability::initial_health(&config);
        let state = RuntimeState {
            config: Arc::new(RwLock::new(config)),
            stats: Arc::new(Mutex::new(RuntimeStats::default())),
            usage,
            health: Arc::new(Mutex::new(HashMap::new())),
            image_capabilities: Arc::new(Mutex::new(images::Ledger::default())),
            ws_health: Arc::new(Mutex::new(ws_health)),
            ws_sessions: Arc::new(Mutex::new(HashMap::new())),
            rr_counters: Arc::new(Mutex::new(HashMap::new())),
            session_affinity: Arc::new(Mutex::new(HashMap::new())),
            clients: Arc::new(Mutex::new(HashMap::new())),
            config_changed: tokio::sync::watch::channel(Arc::new(HashMap::new())).0,
            in_flight_requests: Arc::new(AtomicU64::new(0)),
            target_activity: Arc::new(Mutex::new(HashMap::new())),
            provider_activity: Arc::new(Mutex::new(HashMap::new())),
        };
        let thread_state = state.clone();
        let (stop_tx, stop_rx) = oneshot::channel();
        let (ready_tx, ready_rx) = std::sync::mpsc::sync_channel(1);
        let thread = std::thread::Builder::new().name("aipass-proxy".into()).spawn(move || {
            let runtime = match tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build() {
                Ok(runtime) => runtime,
                Err(err) => { let _ = ready_tx.send(Err(err.to_string())); return; }
            };
            runtime.block_on(async move {
                let listener = match TcpListener::bind(socket).await {
                    Ok(listener) => { let _ = ready_tx.send(Ok(())); listener }
                    Err(err) => { set_error(&thread_state, err.to_string()); let _ = ready_tx.send(Err(err.to_string())); return; }
                };
                let mut stop_rx = stop_rx;
                loop {
                    tokio::select! {
                        _ = &mut stop_rx => break,
                        result = listener.accept() => match result {
                            Ok((stream, _)) => {
                                let state = thread_state.clone();
                                tokio::spawn(async move {
                                    let service = service_fn(move |request| handle_request(request, state.clone()));
                                    let io = TokioIo::new(stream);
                                    let _ = hyper::server::conn::http1::Builder::new().serve_connection(io, service).with_upgrades().await;
                                });
                            }
                            Err(err) => { set_error(&thread_state, err.to_string()); break; }
                        }
                    }
                }
            });
        }).map_err(ProxyError::Io)?;
        match ready_rx.recv_timeout(Duration::from_secs(3)) {
            Ok(Ok(())) => {}
            Ok(Err(err)) => {
                let _ = thread.join();
                return Err(ProxyError::InvalidConfig(format!(
                    "failed to start proxy listener: {err}"
                )));
            }
            Err(err) => {
                let _ = stop_tx.send(());
                let _ = thread.join();
                return Err(ProxyError::InvalidConfig(format!(
                    "proxy listener startup timed out: {err}"
                )));
            }
        }
        state
            .usage
            .log_diagnostic("info", "event=proxy.started".into());
        Ok(Self {
            state,
            stop: Some(stop_tx),
            thread: Some(thread),
            bind_addr,
        })
    }

    pub fn status(&self) -> ProxyStatus {
        let config = self
            .state
            .config
            .read()
            .map(|config| {
                let now = Instant::now();
                let health = self.state.health.lock().ok();
                let activity = self.state.target_activity.lock().ok();
                let channels = config
                    .routes
                    .iter()
                    .filter(|route| route.config.enabled)
                    .flat_map(|route| {
                        route
                            .targets
                            .iter()
                            .filter(|target| target.config.enabled)
                            .map(|target| {
                                let health = health
                                    .as_ref()
                                    .and_then(|health| health.get(&target.config.id));
                                let cooldown = health
                                    .and_then(|health| health.open_until)
                                    .map(|until| until.saturating_duration_since(now))
                                    .unwrap_or_default();
                                ChannelStatus {
                                    route_id: route.config.id,
                                    target_id: target.config.id,
                                    provider_entry_id: target.config.provider_entry_id,
                                    secret_id: target.config.secret_id.clone(),
                                    in_flight_requests: activity
                                        .as_ref()
                                        .and_then(|activity| activity.get(&target.config.id))
                                        .copied()
                                        .unwrap_or_default(),
                                    degraded: health.is_some_and(TargetHealth::degraded),
                                    available: cooldown.is_zero(),
                                    cooldown_remaining_ms: cooldown.as_millis() as u64,
                                    websocket_cooling_down: false,
                                }
                            })
                    })
                    .collect::<Vec<_>>();
                (
                    config.enabled,
                    config
                        .routes
                        .iter()
                        .filter(|route| route.config.enabled)
                        .count(),
                    channels,
                )
            })
            .unwrap_or_default();
        let stats = self.state.stats.lock();
        let (
            requests,
            failures,
            last_error,
            average_first_token_ms,
            recent_requests,
            recent_failures,
            recent_tokens,
        ) = stats
            .as_ref()
            .map(|stats| {
                let cutoff = Instant::now() - Duration::from_secs(60);
                let mut recent_request_times = stats.recent_request_times.clone();
                while recent_request_times
                    .front()
                    .is_some_and(|time| *time < cutoff)
                {
                    recent_request_times.pop_front();
                }
                let mut recent_token_totals = stats.recent_token_totals.clone();
                while recent_token_totals
                    .front()
                    .is_some_and(|(time, _)| *time < cutoff)
                {
                    recent_token_totals.pop_front();
                }
                let mut recent_failure_times = stats.recent_failure_times.clone();
                while recent_failure_times
                    .front()
                    .is_some_and(|time| *time < cutoff)
                {
                    recent_failure_times.pop_front();
                }
                let (total, count) = stats
                    .first_token_samples
                    .iter()
                    .flatten()
                    .fold((0_u64, 0_u64), |(total, count), value| {
                        (total.saturating_add(*value), count.saturating_add(1))
                    });
                (
                    stats.requests,
                    stats.failures,
                    stats.last_error.clone(),
                    (count > 0).then(|| total / count),
                    recent_request_times.len() as u64,
                    recent_failure_times.len() as u64,
                    recent_token_totals.iter().map(|(_, tokens)| *tokens).sum(),
                )
            })
            .unwrap_or_default();
        let success_rate_bps = if requests == 0 {
            0
        } else {
            u16::try_from(
                requests
                    .saturating_sub(failures)
                    .saturating_mul(10_000)
                    .saturating_add(requests / 2)
                    .checked_div(requests)
                    .unwrap_or_default(),
            )
            .unwrap_or(10_000)
        };
        let running = self
            .thread
            .as_ref()
            .is_some_and(|thread| !thread.is_finished());
        let mut channels = config.2;
        if !running {
            for channel in &mut channels {
                channel.in_flight_requests = 0;
                channel.available = false;
                channel.degraded = false;
                channel.cooldown_remaining_ms = 0;
                channel.websocket_cooling_down = false;
            }
        }
        let degraded_target_ids = channels
            .iter()
            .filter(|channel| channel.degraded)
            .map(|channel| channel.target_id)
            .collect::<Vec<_>>();
        let total_channels = channels.len();
        let available_channels = channels.iter().filter(|channel| channel.available).count();
        let degraded = running
            && (!degraded_target_ids.is_empty()
                || (recent_requests > 0
                    && recent_failures.saturating_mul(100) >= recent_requests.saturating_mul(20)));
        ProxyStatus {
            running,
            enabled: config.0,
            bind_addr: self.bind_addr.clone(),
            active_routes: config.1,
            requests,
            failures,
            last_error,
            degraded,
            degraded_target_ids,
            recent_requests,
            recent_tokens,
            success_rate_bps,
            average_first_token_ms,
            in_flight_requests: self.state.in_flight_requests.load(Ordering::Relaxed),
            available_channels,
            total_channels,
            channels,
        }
    }

    pub fn update_config(&self, config: RuntimeConfig) -> Result<(), ProxyError> {
        let mut current = self
            .state
            .config
            .write()
            .map_err(|_| ProxyError::Poisoned)?;
        if *current == config {
            // Syncing unrelated vault records must not invalidate live HTTP,
            // SSE, WS sessions, pooled connections, or routing health.
            return Ok(());
        }
        let mut health = self.state.health.lock().map_err(|_| ProxyError::Poisoned)?;
        let mut ws_health = self
            .state
            .ws_health
            .lock()
            .map_err(|_| ProxyError::Poisoned)?;
        let mut rr_counters = self
            .state
            .rr_counters
            .lock()
            .map_err(|_| ProxyError::Poisoned)?;
        let mut session_affinity = self
            .state
            .session_affinity
            .lock()
            .map_err(|_| ProxyError::Poisoned)?;
        let preserved = routing::preserved_targets(&current, &config);
        health.retain(|id, _| preserved.iter().any(|(_, target)| target == id));
        session_affinity
            .retain(|(route, _), affinity| preserved.contains(&(*route, affinity.target_id)));
        // Transport sessions still reconnect and authenticate against the new
        // snapshot, while unrelated metadata edits retain generation stability.
        let valid_ws_keys: HashSet<_> = config
            .routes
            .iter()
            .flat_map(|route| &route.targets)
            .map(|target| websocket_config_key(target, &config.upstream_proxy))
            .collect();
        ws_health.retain(|key, _| valid_ws_keys.contains(key));
        for target in config
            .routes
            .iter()
            .flat_map(|route| &route.targets)
            .filter(|target| target.supports_websockets)
        {
            if current
                .routes
                .iter()
                .flat_map(|route| &route.targets)
                .any(|old| {
                    old.config.provider_entry_id == target.config.provider_entry_id
                        && !old.supports_websockets
                })
            {
                ws_health.remove(&websocket_config_key(target, &config.upstream_proxy));
            }
        }
        for key in &valid_ws_keys {
            ws_health.entry(*key).or_default();
        }
        if let Ok(mut sessions) = self.state.ws_sessions.lock() {
            sessions.retain(|(_, _, key), _| valid_ws_keys.contains(key));
        }
        rr_counters.clear();
        let mut generations = (**self.state.config_changed.borrow()).clone();
        *generations.entry(Uuid::nil()).or_default() += 1;
        for route in &current.routes {
            let revoked = current.enabled != config.enabled
                || current.bind_addr != config.bind_addr
                || route
                    .targets
                    .iter()
                    .any(|target| !preserved.contains(&(route.config.id, target.config.id)));
            if revoked {
                *generations.entry(route.config.id).or_default() += 1;
            }
        }
        *current = config;
        self.state
            .config_changed
            .send_replace(Arc::new(generations));
        self.state
            .usage
            .log_diagnostic("info", "event=proxy.config.reloaded".into());
        Ok(())
    }

    pub fn logs(&self) -> Result<Vec<ProxyLogEntry>, ProxyError> {
        self.state.usage.logs()
    }

    pub fn usage_count(&self) -> Result<u64, ProxyError> {
        self.state.usage.count()
    }
}

impl Drop for ProxyHandle {
    fn drop(&mut self) {
        if let Some(stop) = self.stop.take() {
            let _ = stop.send(());
        }
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
        self.state
            .usage
            .log_diagnostic("info", "event=proxy.stopped".into());
    }
}
