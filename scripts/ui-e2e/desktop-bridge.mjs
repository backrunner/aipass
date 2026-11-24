// The production entry point runs unchanged. Only the native IPC boundary is substituted.
export async function installDesktop(page, options = {}) {
  await page.addInitScript((options) => {
    const state = window.desktopFixture = {
      status: { exists: true, locked: false, ...options.status },
      preferences: { theme: "light", locale: "en", ...options.preferences },
      sync: { mode: "local", hasWebdavPassword: false },
      calls: [],
      unhandled: [],
    };
    const callbacks = new Map();
    const listeners = new Map();
    let nextId = 1;
    state.emit = (event, payload) => {
      for (const id of listeners.get(event) ?? []) callbacks.get(id)?.({ id, event, payload });
    };
    window.__TAURI_EVENT_PLUGIN_INTERNALS__ = {
      unregisterListener(event, id) { listeners.get(event)?.delete(id); },
    };
    window.__TAURI_INTERNALS__ = {
      metadata: { currentWindow: { label: "main" }, currentWebview: { label: "main" } },
      transformCallback(fn) { const id = nextId++; callbacks.set(id, fn); return id; },
      unregisterCallback(id) { callbacks.delete(id); },
      async invoke(command, args = {}) {
        state.calls.push(command);
        const custom = await window.__desktopBusinessInvoke?.(command, args);
        if (custom !== undefined) return custom;
        switch (command) {
          case "plugin:event|listen": {
            if (!listeners.has(args.event)) listeners.set(args.event, new Set());
            listeners.get(args.event).add(args.handler);
            return args.handler;
          }
          case "plugin:window|is_focused":
          case "window_compositing": return true;
          case "plugin:window|is_maximized":
          case "install_pending_update": return false;
          case "plugin:app|version": return "0.2.0";
          case "window_target": return "main";
          case "vault_status":
          case "session_touch": return { ...state.status };
          case "vault_lock": state.status.locked = true; return { ...state.status };
          case "language_settings_load":
          case "preferences_load": return state.preferences;
          case "preferences_save": return state.preferences = { ...args.request, resolvedLocale: args.request.locale === "zh-CN" ? "zh-CN" : "en" };
          case "sync_settings_load": return state.sync;
          case "sync_settings_save": return state.sync = { ...args.request, hasWebdavPassword: !!args.request.webdavPassword };
          case "check_for_updates": return { available: false, currentVersion: "0.2.0" };
          case "browser_extension_status": return { chromeInstalled: true, zipExists: true, extensionInstalled: true, nativeHostConfigured: true };
          case "ccswitch_detect": return { configExists: false, appInstalled: false };
          case "control_panel_status": return { settings: { enabled: false, address: "127.0.0.1", port: 8788, https: false }, addresses: ["127.0.0.1"], running: false };
          case "server_config_get": return { enabled: false, bindAddr: "127.0.0.1:8787", routes: [], pricing: [], upstreamProxy: { mode: "system" } };
          case "server_status": return { enabled: false, running: false, bindAddr: "127.0.0.1:8787" };
          case "pricing_config_get": return { groups: [], assignments: [] };
          case "server_usage_summary": return { requestCount: 0, inputTokens: 0, outputTokens: 0, estimatedCostMicros: 0, providers: [], models: [] };
          case "devices_list":
          case "sync_conflicts":
          case "entries_list":
          case "entries_favorites_list":
          case "entries_trash_list":
          case "server_usage_timeseries":
          case "take_pending_deep_links":
          case "tool_config_detect": return [];
          case "clipboard_clear_sensitive":
          case "desktop_ready":
          case "desktop_startup_stage":
          case "plugin:event|unlisten": return null;
          default:
            state.unhandled.push(command);
            throw new Error("Missing desktop fixture for " + command);
        }
      },
    };
  }, options);
}
