declare namespace chrome {
  namespace runtime {
    const id: string;
    const lastError: { message?: string } | undefined;
    function sendNativeMessage(
      application: string,
      message: unknown,
      callback: (response: unknown) => void
    ): void;
    function sendMessage(message: unknown, callback?: (response: unknown) => void): void;
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
  namespace tabs {
    function query(queryInfo: { active: boolean; currentWindow: boolean }, callback: (tabs: Array<{ id?: number; url?: string }>) => void): void;
    function sendMessage(tabId: number, message: unknown, callback?: (response: unknown) => void): void;
  }
  namespace storage {
    const local: {
      get(keys: string[] | string | Record<string, unknown> | null, callback: (items: Record<string, unknown>) => void): void;
      set(items: Record<string, unknown>, callback?: () => void): void;
    };
  }
}
