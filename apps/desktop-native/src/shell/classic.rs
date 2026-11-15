//! Windows native menu and Linux AppIndicator, without a webview event loop.
use super::{i18n::tr, status::TrayStatusDto, Handle};
use anyhow::Result;
use tray_icon::{
    menu::{CheckMenuItem, Menu, MenuEvent, MenuItem, Submenu},
    Icon, TrayIcon, TrayIconBuilder,
};

struct NativeTray {
    icon: TrayIcon,
    menu: Menu,
    handle: Handle,
}
impl NativeTray {
    fn new(handle: Handle) -> Result<Self> {
        let image =
            image::load_from_memory(include_bytes!("../../bundle/icons/icon.png"))?.into_rgba8();
        let (width, height) = image.dimensions();
        let icon = Icon::from_rgba(image.into_raw(), width, height)?;
        let menu = Menu::new();
        menu.append(&MenuItem::with_id("open", tr("ext.openApp"), true, None))?;
        menu.append(&MenuItem::with_id("quit", tr("tray.quit"), true, None))?;
        let icon = TrayIconBuilder::new()
            .with_icon(icon)
            .with_tooltip("AIPass")
            .with_menu(Box::new(menu.clone()))
            .build()?;
        Ok(Self { icon, menu, handle })
    }
    fn update(&self, value: &TrayStatusDto) -> Result<()> {
        super::i18n::set_locale(value.locale);
        // Keep the menu instance alive: Linux indicators do not support replacement.
        for index in (0..self.menu.items().len()).rev() {
            self.menu.remove_at(index);
        }
        self.menu
            .append(&MenuItem::new(&value.agent_text, false, None))?;
        for (id, key, enabled) in [
            ("open", "ext.openApp", true),
            ("hide", "tray.hideWindow", true),
            ("start-agent", "tray.startAgent", value.can_start_agent),
            ("lock-vault", "titlebar.lock", value.can_lock),
            ("refresh", "ext.refresh", true),
            (
                "repair-autostart",
                "tray.repairAutostart",
                self.handle.supervise,
            ),
        ] {
            self.menu
                .append(&MenuItem::with_id(id, tr(key), enabled, None))?;
        }
        let proxy = Submenu::new(tr("tray.proxy"), true);
        proxy.append(&MenuItem::new(&value.proxy_text, false, None))?;
        for (id, key, enabled) in [
            ("proxy-open", "tray.openServer", value.can_open_proxy),
            ("proxy-start", "tray.startProxy", value.can_start_proxy),
            ("proxy-stop", "tray.stopProxy", value.can_stop_proxy),
        ] {
            proxy.append(&MenuItem::with_id(id, tr(key), enabled, None))?;
        }
        let groups = Submenu::new(tr("tray.switchGroup"), !value.proxy_groups.is_empty());
        for group in &value.proxy_groups {
            groups.append(&CheckMenuItem::with_id(
                format!("proxy-group-{}", group.id),
                &group.name,
                true,
                group.active,
                None,
            ))?;
        }
        proxy.append(&groups)?;
        self.menu.append(&proxy)?;
        let panel = Submenu::new(tr("tray.panel"), true);
        panel.append(&MenuItem::new(
            value
                .panel_url
                .clone()
                .unwrap_or_else(|| tr("tray.panelOff")),
            false,
            None,
        ))?;
        for (id, key) in [
            ("panel-open-url", "tray.panelOpen"),
            ("panel-copy", "tray.panelCopy"),
            ("panel-stop", "tray.panelStop"),
        ] {
            panel.append(&MenuItem::with_id(
                id,
                tr(key),
                value.panel_url.is_some(),
                None,
            ))?;
        }
        self.menu.append(&panel)?;
        self.menu
            .append(&MenuItem::with_id("quit", tr("tray.quit"), true, None))?;
        self.icon.set_tooltip(Some(&value.tooltip))?;
        Ok(())
    }
    fn poll(&self) {
        while let Ok(event) = MenuEvent::receiver().try_recv() {
            super::tray::action(&self.handle, event.id.as_ref());
        }
        while tray_icon::TrayIconEvent::receiver().try_recv().is_ok() {
            self.handle.refresh();
        }
    }
}

pub struct Tray {
    #[cfg(target_os = "linux")]
    sender: std::sync::mpsc::SyncSender<Option<TrayStatusDto>>,
    #[cfg(not(target_os = "linux"))]
    inner: NativeTray,
}
impl Tray {
    pub fn new(handle: Handle) -> Result<Self> {
        #[cfg(target_os = "linux")]
        {
            let (sender, receiver) = std::sync::mpsc::sync_channel::<Option<TrayStatusDto>>(2);
            let (ready, wait) = std::sync::mpsc::sync_channel(1);
            std::thread::spawn(move || {
                let tray = gtk::init()
                    .map_err(|error| anyhow::anyhow!(error.to_string()))
                    .and_then(|()| NativeTray::new(handle));
                let tray = match tray {
                    Ok(tray) => {
                        let _ = ready.send(Ok(()));
                        tray
                    }
                    Err(error) => {
                        let _ = ready.send(Err(error.to_string()));
                        return;
                    }
                };
                gtk::glib::timeout_add_local(std::time::Duration::from_millis(50), move || {
                    loop {
                        match receiver.try_recv() {
                            Ok(Some(status)) => {
                                let _ = tray.update(&status);
                            }
                            Ok(None) | Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                                gtk::main_quit();
                                return gtk::glib::ControlFlow::Break;
                            }
                            Err(std::sync::mpsc::TryRecvError::Empty) => break,
                        }
                    }
                    tray.poll();
                    gtk::glib::ControlFlow::Continue
                });
                gtk::main();
            });
            wait.recv_timeout(std::time::Duration::from_secs(5))?
                .map_err(anyhow::Error::msg)?;
            Ok(Self { sender })
        }
        #[cfg(not(target_os = "linux"))]
        {
            Ok(Self {
                inner: NativeTray::new(handle)?,
            })
        }
    }
    pub fn update(&self, status: &TrayStatusDto) {
        #[cfg(target_os = "linux")]
        let _ = self.sender.try_send(Some(status.clone()));
        #[cfg(not(target_os = "linux"))]
        let _ = self.inner.update(status);
    }
    pub fn poll(&self) {
        #[cfg(not(target_os = "linux"))]
        self.inner.poll();
    }
}
#[cfg(target_os = "linux")]
impl Drop for Tray {
    fn drop(&mut self) {
        let _ = self.sender.try_send(None);
    }
}
