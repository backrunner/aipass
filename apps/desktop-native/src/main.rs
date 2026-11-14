mod agents_ui;
mod appearance;
mod assets;
mod auth;
mod backdrop;
mod composite;
mod curtain;
mod desktop;
mod disclosure;
mod form_ui;
mod integrations;
mod locale;
mod logo;
mod menu;
mod parity;
mod preferences;
mod provider_icon;
mod provider_tools_ui;
mod providers;
mod recovery_kit;
mod scroll_fade;
mod subscription_ui;
mod theme;
mod toast;
mod trash_ui;
mod typography;
mod update_ui;
#[cfg(feature = "parity-render")]
mod update_validation;
mod workspace;

use aipass_desktop_native::session::{AgentTransport, Session};
use desktop::Desktop;
use gpui::{prelude::*, px, size, App, Bounds, WindowBounds, WindowOptions};

fn main() {
    typography::init();
    let args: Vec<String> = std::env::args().skip(1).collect();
    #[cfg(feature = "parity-render")]
    let args = parity::launch::arguments(args).unwrap_or_else(|error| {
        eprintln!("Invalid isolated launch fixture: {error}");
        std::process::exit(2);
    });
    if args.iter().any(|arg| arg == "--motion-samples") {
        let samples: Vec<_> = [0.0, 60.0, 135.0, 270.0, 405.0, 539.0, 540.0, 600.0,
            660.0, 661.0, 795.0, 930.0, 1065.0, 1200.0].into_iter().map(|ms| {
                serde_json::json!({"ms": ms, "offset": aipass_desktop_native::motion::unlock_offset(ms, false, false)})
            }).collect();
        println!(
            "{}",
            serde_json::to_string(&samples).expect("finite animation samples")
        );
        return;
    }
    if !args.iter().any(|arg| arg == "--parity-preview") {
        eprintln!("GPUI migration is not yet a production desktop. Use --parity-preview for isolated UI comparison.");
        std::process::exit(2);
    }
    #[cfg(not(feature = "parity-render"))]
    if args.iter().any(|arg| arg.starts_with("--update-config=")) {
        eprintln!("Update fixtures require parity-render");
        std::process::exit(2);
    }
    #[cfg(feature = "parity-render")]
    let mut updater = update_validation::load(&args).unwrap_or_else(|error| {
        eprintln!("Could not start isolated updater: {error:#}");
        std::process::exit(2);
    });
    #[cfg(feature = "parity-render")]
    let appearance_samples = parity::appearance::parse(&args).unwrap_or_else(|error| {
        eprintln!("Invalid appearance capture: {error:#}");
        std::process::exit(2);
    });
    let dark = args.iter().any(|arg| arg == "--dark");
    let chinese = args.iter().any(|arg| arg == "--zh-CN");
    let large = args.iter().any(|arg| arg == "--large");
    let reduced = args.iter().any(|arg| arg == "--reduced-motion");
    #[cfg(feature = "parity-render")]
    let backdrop_at = args
        .iter()
        .find_map(|arg| arg.strip_prefix("--backdrop-at="))
        .map(|value| {
            value
                .parse::<f64>()
                .ok()
                .filter(|n| n.is_finite() && (0.0..=60000.0).contains(n))
                .unwrap_or_else(|| {
                    eprintln!("--backdrop-at must be within 0..60000 milliseconds");
                    std::process::exit(2);
                })
        });
    #[cfg(feature = "parity-render")]
    if backdrop_at.is_some()
        && (args
            .iter()
            .any(|arg| arg.starts_with("--vault=") || arg.starts_with("--update-config="))
            || !args.iter().any(|arg| arg.starts_with("--capture=")))
    {
        eprintln!("Background time samples require an isolated synthetic capture");
        std::process::exit(2);
    }
    #[cfg(feature = "parity-render")]
    let motion_at = args
        .iter()
        .find_map(|arg| arg.strip_prefix("--motion-at="))
        .map(|value| {
            value
                .parse::<f64>()
                .ok()
                .filter(|n| n.is_finite() && (0.0..=1200.0).contains(n))
                .unwrap_or_else(|| {
                    eprintln!("--motion-at must be within 0..1200 milliseconds");
                    std::process::exit(2);
                })
        });
    #[cfg(feature = "parity-render")]
    let capture = args
        .iter()
        .find_map(|arg| arg.strip_prefix("--capture="))
        .map(std::path::PathBuf::from);
    #[cfg(feature = "parity-render")]
    let capture_actions: Vec<String> = args
        .iter()
        .filter_map(|arg| arg.strip_prefix("--capture-action=").map(str::to_owned))
        .collect();
    #[cfg(feature = "parity-render")]
    if !capture_actions.is_empty() && capture.is_none() {
        eprintln!("Control actions require an isolated synthetic capture");
        std::process::exit(2);
    }
    if args
        .iter()
        .any(|arg| arg.starts_with("--capture=") || arg.starts_with("--motion-trace="))
        && args.iter().any(|arg| arg.starts_with("--vault="))
    {
        eprintln!("Capture mode accepts synthetic fixtures only.");
        std::process::exit(2);
    }
    let scenario = args
        .iter()
        .find_map(|arg| arg.strip_prefix("--scenario="))
        .unwrap_or("empty")
        .to_owned();
    #[cfg(feature = "parity-render")]
    let subscription_fixture = subscription_ui::fixtures::SCENARIOS.contains(&scenario.as_str());
    #[cfg(not(feature = "parity-render"))]
    let subscription_fixture = false;
    #[cfg(feature = "parity-render")]
    let agent_fixture = agents_ui::fixtures::SCENARIOS.contains(&scenario.as_str());
    #[cfg(not(feature = "parity-render"))]
    let agent_fixture = false;
    if !subscription_fixture
        && !agent_fixture
        && !matches!(
            scenario.as_str(),
            "empty"
                | "providers"
                | "providers-reset"
                | "providers-reset-error"
                | "providers-usage"
                | "providers-usage-advanced"
                | "providers-usage-result"
                | "providers-usage-failed"
                | "providers-usage-busy"
                | "providers-probe-result"
                | "providers-notes-normal"
                | "providers-notes-empty"
                | "providers-notes-long"
                | "providers-list"
                | "providers-list-hover"
                | "providers-list-selected-hover"
                | "providers-add"
                | "providers-add-menu"
                | "providers-add-all"
                | "providers-edit"
                | "providers-add-advanced"
                | "providers-edit-advanced"
                | "providers-edit-fields"
                | "providers-edit-gateway"
                | "providers-key-add"
                | "providers-key-edit"
                | "providers-key-billing"
                | "providers-key-reveal"
                | "providers-pricing"
                | "providers-pricing-reference"
                | "providers-pricing-source"
                | "providers-pricing-group"
                | "providers-pricing-confirm"
                | "providers-pricing-delete"
                | "providers-pricing-history"
                | "providers-pricing-offpeak"
                | "providers-trash"
                | "providers-trash-delete"
                | "providers-trash-empty"
                | "providers-integration-picker"
                | "providers-integration-diff"
                | "providers-integration-full"
                | "providers-integration-file2"
                | "providers-integration-unchanged"
                | "providers-integration-readonly"
                | "locked"
                | "locked-busy"
                | "locked-error"
                | "boot"
                | "setup-cloud"
                | "setup-cloud-failed"
                | "create-local"
                | "recovery-kit"
                | "recovery-kit-confirmed"
                | "setup-import-backup"
                | "setup-import-vault"
                | "setup-import-local"
                | "setup-import-icloud"
                | "setup-import-webdav"
                | "setup"
                | "create"
                | "recover"
                | "reset"
                | "reset-bottom"
                | "menu"
                | "filter"
                | "toast"
                | "toast-long"
                | "unlock-motion"
                | "lock-motion"
        )
    {
        eprintln!("Unknown comparison scenario: {scenario}");
        std::process::exit(2);
    }
    if (scenario.ends_with("-motion")
        || scenario.starts_with("providers")
        || scenario.starts_with("agents-")
        || scenario.starts_with("subscription-")
        || scenario.starts_with("setup-")
        || scenario.starts_with("recovery-kit")
        || scenario == "create-local"
        || scenario == "boot")
        && args.iter().any(|arg| arg.starts_with("--vault="))
    {
        eprintln!("Presentation fixtures accept synthetic data only");
        std::process::exit(2);
    }
    if matches!(
        scenario.as_str(),
        "create"
            | "recover"
            | "reset"
            | "reset-bottom"
            | "locked-busy"
            | "locked-error"
            | "menu"
            | "filter"
            | "toast"
            | "toast-long"
    ) && args.iter().any(|arg| arg.starts_with("--vault="))
    {
        eprintln!("Presentation fixtures accept synthetic data only");
        std::process::exit(2);
    }
    #[cfg(feature = "parity-render")]
    if motion_at.is_some() && !scenario.ends_with("-motion") {
        eprintln!("--motion-at requires an isolated motion scenario");
        std::process::exit(2);
    }
    #[cfg(not(feature = "parity-render"))]
    if scenario.starts_with("providers")
        || args.iter().any(|arg| {
            [
                "--capture=",
                "--capture-action=",
                "--appearance-sample=",
                "--motion-at=",
                "--motion-trace=",
                "--backdrop-at=",
            ]
            .iter()
            .any(|prefix| arg.starts_with(prefix))
        })
    {
        eprintln!(
            "Provider fixtures, capture and motion observations require the parity-render feature"
        );
        std::process::exit(2);
    }
    #[cfg(feature = "parity-render")]
    let motion_trace = args
        .iter()
        .find_map(|arg| arg.strip_prefix("--motion-trace="))
        .map(|path| {
            std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(path)
                .unwrap_or_else(|error| {
                    eprintln!("Could not create motion trace: {error}");
                    std::process::exit(2);
                })
        });
    let session = args
        .iter()
        .find_map(|arg| arg.strip_prefix("--vault="))
        .map(|path| {
            AgentTransport::for_vault(path.into())
                .and_then(|transport| Session::new(transport).map_err(Into::into))
        })
        .transpose()
        .unwrap_or_else(|error| {
            eprintln!("Could not initialize Agent client: {error}");
            std::process::exit(2);
        });
    #[cfg(feature = "parity-render")]
    let session = updater
        .as_mut()
        .and_then(|context| context.session.take())
        .or(session);
    let preferences = preferences::load(&args, session.is_some()).unwrap_or_else(|error| {
        eprintln!("Could not load desktop preferences: {error}");
        std::process::exit(2);
    });
    gpui_platform::application()
        .with_assets(assets::Assets)
        .run(move |cx: &mut App| {
            gpui_base::init(cx);
            cx.set_global(preferences);
            // Explicit fixture colors stay deterministic; normal windows honor
            // the shared preference, whose default is the system appearance.
            let fixed_appearance = args.iter().any(|arg| matches!(arg.as_str(), "--dark" | "--light") || arg.starts_with("--capture="));
            let preference = if fixed_appearance {
                if dark { appearance::ThemePreference::Dark } else { appearance::ThemePreference::Light }
            } else { cx.global::<preferences::Preferences>().0.theme };
            cx.set_window_appearance(appearance::forced(preference));
            #[cfg(feature = "parity-render")]
            cx.set_global(parity::Backgrounds { time_ms: backdrop_at, ..Default::default() });
            cx.text_system().add_fonts(vec![std::borrow::Cow::Borrowed(include_bytes!("../assets/password-dots.ttf"))]).expect("embedded password glyph font");
            // Register the full CJK family before CoreText discovers a single
            // fallback face while shaping a heading.
            cx.text_system().prewarm_fonts(&[gpui::font("PingFang SC")]);
            desktop::init(cx);
            cx.on_app_quit(|cx| {
                aipass_desktop_platform::clipboard::invalidate();
                cx.background_executor().spawn(async {
                    let _ = aipass_desktop_platform::clipboard::clear_now();
                })
            }).detach();
            #[cfg(feature = "parity-render")]
            if let Some(output) = motion_trace {
                cx.set_global(parity::MotionTrace { output: Some(output), frames: Vec::new() });
            }
            if reduced { cx.set_reduce_motion(true); }
            let dimensions = if large {
                size(px(1280.), px(820.))
            } else {
                size(px(960.), px(640.))
            };
            let bounds = Bounds::centered(None, dimensions, cx);
            let screenshot_only = args.iter().any(|arg| arg.starts_with("--capture="));
            #[cfg(feature = "parity-render")]
            let mut desktop = None;
            let result = cx.open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(bounds)),
                    window_min_size: Some(size(px(960.), px(640.))),
                    focus: !screenshot_only,
                    show: !screenshot_only,
                    titlebar: None,
                    window_background: gpui::WindowBackgroundAppearance::Transparent,
                    app_id: Some("com.alkinum.aipass.gpui-parity".into()),
                    ..Default::default()
                },
                |window, cx| {
                    window.set_window_title("AIPass GPUI Parity");
                    #[cfg(feature = "parity-render")]
                    if session.is_none() {
                        if let Err(error) = parity::launch::observe(window) {
                            eprintln!("Could not create native-window evidence: {error:#}");
                        }
                    }
                    let dark = appearance::resolve(preference, window.appearance());
                    let view = cx.new(|cx| Desktop::new(session, &scenario, dark, chinese, window, cx));
                    if !fixed_appearance {
                        view.update(cx, |view, cx| view.follow_preferences(window, cx));
                    }
                    #[cfg(feature = "parity-render")]
                    if let Some(context) = updater.take() {
                        let view = view.downgrade();
                        let restart_args = args.clone();
                        let ready_path = std::env::var_os("AIPASS_NATIVE_UPDATE_READY");
                        std::env::remove_var("AIPASS_NATIVE_UPDATE_READY");
                        window.on_next_frame(move |_, cx| {
                            // Readiness requires a rendered frame and typed Agent readiness.
                            // Do network/IPC work off the native event loop.
                            let ready_services = context.services.clone();
                            if let Some(path) = ready_path {
                                std::thread::spawn(move || {
                                    let ready = ready_services.as_ref().map(|services| services.ensure_ready()).transpose();
                                    if ready.is_ok() {
                                        let _ = aipass_storage::atomic_write_bytes(std::path::PathBuf::from(path), std::process::id().to_string().as_bytes());
                                    }
                                });
                            }
                            let gate = context.host.runtime.as_ref().map(|runtime| runtime.gate());
                            match aipass_desktop_native::updates::Worker::with_host(context.updater, restart_args, context.host) {
                                Ok(worker) => { let _ = view.update(cx, |view, cx| { view.enable_updates(worker, gate); cx.notify(); }); }
                                Err(error) => eprintln!("Could not start update worker: {error:#}"),
                            }
                        });
                    }
                    #[cfg(feature = "parity-render")]
                    if let Some(ms) = motion_at { view.update(cx, |view, cx| view.freeze_motion_at(ms, cx)); }
                    #[cfg(feature = "parity-render")]
                    {
                        desktop = Some(view.clone());
                    }
                    cx.new(|cx| gpui_base::Root::new(view, window, cx))
                },
            );
            if let Err(ref error) = result {
                eprintln!("Could not open GPUI comparison window: {error}");
                cx.quit();
            }
            if let Ok(handle) = &result {
                // Apply content dimensions after macOS has created its frame.
                // A borderless initial frame otherwise adds a logical pixel.
                if let Err(error) = handle.update(cx, |_, window, _| window.resize(dimensions)) {
                    eprintln!("Could not size GPUI comparison window: {error}");
                    cx.quit();
                }
            }
            #[cfg(feature = "parity-render")]
            if let (Ok(handle), Some(path), Some(desktop)) = (result, capture, desktop) {
                cx.spawn(async move |cx| {
                    if let Err(error) = cx.update_window(handle.into(), |_, window, _| window.resize(dimensions)) {
                        eprintln!("Capture resize failed: {error}");
                        std::process::exit(1);
                    }
                    cx.background_executor().timer(std::time::Duration::from_millis(500)).await;
                    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(15);
                    loop {
                        if let Err(error) = cx.update_window(handle.into(), |_, window, cx| window.draw(cx).clear(cx)) {
                            eprintln!("Capture draw failed: {error}"); std::process::exit(1);
                        }
                        match desktop.read_with(cx, |desktop, cx| desktop.capture_ready(cx)) {
                            Ok(true) => break,
                            Ok(false) if std::time::Instant::now() < deadline => {},
                            other => { eprintln!("Capture assets not ready: {other:?}"); std::process::exit(1); }
                        }
                        cx.background_executor().timer(std::time::Duration::from_millis(50)).await;
                    }
                    let result = cx.update_window(handle.into(), |_, window, cx| {
                        // Match the reference's final-viewport scroll operation,
                        // after macOS sizing and background preparation settle.
                        desktop.read(cx).prepare_capture_scroll(&scenario, cx);
                        window.dispatch_event(gpui::PlatformInput::MouseMove(gpui::MouseMoveEvent { position: gpui::point(px(-1.),px(-1.)), ..Default::default() }),cx);
                        window.draw(cx).clear(cx);
                        if scenario.starts_with("providers-list") && scenario.ends_with("-hover") {
                            let bounds = &cx.global::<parity::Geometry>().0["provider-list-row-1"];
                            let position = gpui::point(px(bounds[0] + bounds[2] / 2.), px(bounds[1] + bounds[3] / 2.));
                            window.dispatch_event(gpui::PlatformInput::MouseMove(gpui::MouseMoveEvent { position, ..Default::default() }),cx);
                            window.draw(cx).clear(cx);
                            window.draw(cx).clear(cx);
                        }
                        for action in &capture_actions { parity::control_actions::apply(action, window, cx)?; }
                        Ok::<_, anyhow::Error>(())
                    });
                    if let Err(error) = result.and_then(|r| r) {
                        eprintln!("Capture setup failed: {error}");
                        std::process::exit(1);
                    }
                    if let Err(error) = parity::appearance::apply(&appearance_samples, handle, &desktop, cx).await {
                        eprintln!("Appearance capture failed: {error:#}");
                        std::process::exit(1);
                    }
                    let result = cx.update_window(handle.into(), |_, window, cx| {
                        let image = window.render_to_image()?;
                        #[cfg(target_os = "macos")]
                        let image = {
                            let mut image = image;
                            image.save(path.with_extension("metal-rgba.png"))?;
                            parity::straight_alpha(image.as_mut());
                            image
                        };
                        image.save(&path)?;
                        let metadata = serde_json::json!({"renderer": "gpui-offscreen", "scale": window.scale_factor(),
                            "viewport": [f32::from(window.viewport_size().width), f32::from(window.viewport_size().height)],
                            "pixels": [image.width(), image.height()],
                            "presentation": desktop.read(cx).presentation_state(cx),
                            "appearance": desktop.read(cx).appearance_state(),
                            "geometry": cx.try_global::<parity::Geometry>().map(|g| &g.0),
                            "styles": cx.try_global::<parity::Styles>().map(|g| &g.0),
                            "scrolls": cx.try_global::<parity::Scrolls>().map(|g| &g.0),
                            "backgrounds": cx.try_global::<parity::Backgrounds>().map(|b|&b.frames),
                            "reducedMotion": cx.reduce_motion(), "motionTimeMs": motion_at,
                            "controlActions": capture_actions,
                            "alphaEncoding": if cfg!(target_os = "macos") { "straight-from-premultiplied-metal" } else { "platform-readback" }});
                        std::fs::write(path.with_extension("json"), serde_json::to_vec_pretty(&metadata)?)?;
                        Ok::<_, anyhow::Error>(())
                    });
                    if let Err(error) = result.and_then(|r| r) {
                        eprintln!("Offscreen capture failed: {error}");
                        std::process::exit(1);
                    }
                    cx.update(|cx| cx.quit());
                }).detach();
            }
            if !screenshot_only { cx.activate(true); }
        });
}

mod pricing_ui;
