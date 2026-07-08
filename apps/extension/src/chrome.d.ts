declare namespace chrome {
  namespace runtime {
    interface Port {
      postMessage(message: unknown): void;
      disconnect(): void;
      onMessage: {
        addListener(callback: (message: unknown) => void): void;
      };
      onDisconnect: {
        addListener(callback: (port: Port) => void): void;
      };
    }

    const id: string;
    const lastError: { message?: string } | undefined;
    const connectNative: ((application: string) => Port) | undefined;
    function getManifest(): { update_url?: string };
    function sendNativeMessage(
      application: string,
      message: unknown,
      callback: (response: unknown) => void
    ): void;
    function sendMessage(message: unknown, callback?: (response: unknown) => void): void;
    const onStartup: { addListener(callback: () => void): void } | undefined;
    const onInstalled: { addListener(callback: () => void): void } | undefined;
    const onMessage: {
      addListener(
        callback: (
          message: unknown,
          sender: unknown,
          sendResponse: (response?: unknown) => void
        ) => boolean | void
      ): void;
    };
  }
  namespace alarms {
    function create(name: string, alarmInfo: { periodInMinutes: number }): void;
    const onAlarm: {
      addListener(callback: (alarm: { name: string }) => void): void;
    };
  }
  namespace tabs {
    function query(
      queryInfo: { active: boolean; currentWindow: boolean },
      callback: (tabs: Array<{ id?: number; url?: string; title?: string }>) => void
    ): void;
    function sendMessage(tabId: number, message: unknown, callback?: (response: unknown) => void): void;
  }
  namespace action {
    function setBadgeText(details: { text: string }): void;
    function setBadgeBackgroundColor(details: { color: string }): void;
    const openPopup: (() => Promise<void>) | undefined;
  }
  namespace scripting {
    function executeScript(details: { target: { tabId: number }; files: string[]; world?: "ISOLATED" | "MAIN" }): Promise<unknown[]>;
  }
  namespace storage {
    interface StorageArea {
      get(keys: string | string[] | Record<string, unknown> | null, callback: (items: Record<string, unknown>) => void): void;
      set(items: Record<string, unknown>, callback?: () => void): void;
      remove?(keys: string | string[], callback?: () => void): void;
    }

    const session: StorageArea | undefined;
  }
}
