import { invoke } from "@tauri-apps/api/core";

import { localizedMessage } from "../stores/i18n";
import type { MessageValue } from "../types";

export type UpdateCheckResult = {
  currentVersion: string;
  available: boolean;
  latestVersion?: string;
  notes?: string;
  error?: MessageValue;
};

const hasTauri = () =>
  typeof window !== "undefined" &&
  Boolean((window as Window & { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__);

export async function checkForUpdates(): Promise<UpdateCheckResult> {
  if (!hasTauri()) {
    return {
      currentVersion: "dev",
      available: false,
      error: localizedMessage("error.updatesUnavailable")
    };
  }
  return invoke<UpdateCheckResult>("check_for_updates");
}

export async function installUpdate(): Promise<void> {
  if (!hasTauri()) throw localizedMessage("error.updatesUnavailable");
  await invoke("install_update");
}
