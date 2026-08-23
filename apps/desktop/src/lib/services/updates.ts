import { invoke } from "@tauri-apps/api/core";

import { localizedMessage } from "../stores/i18n";
import type { MessageValue } from "../types";

export type UpdateChannel = "official" | "beta";

export type UpdateProgress = {
  phase: "downloading" | "installing";
  downloadedBytes: number;
  totalBytes?: number | null;
};

export const UPDATE_PROGRESS_EVENT = "update-progress";

export type UpdateCheckResult = {
  currentVersion: string;
  available: boolean;
  latestVersion?: string;
  notes?: string;
  error?: MessageValue;
};

export const UPDATE_CHANNEL_STORAGE_KEY = "aipass:updateChannel";

const hasTauri = () =>
  typeof window !== "undefined" &&
  Boolean((window as Window & { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__);

export function inferUpdateChannel(version: string): UpdateChannel {
  return version.includes("-") ? "beta" : "official";
}

export function getStoredUpdateChannel(): UpdateChannel | undefined {
  try {
    const stored = localStorage.getItem(UPDATE_CHANNEL_STORAGE_KEY);
    if (stored === "official" || stored === "beta") return stored;
  } catch {
    // Ignore storage failures; fall back to the inferred channel.
  }
  return undefined;
}

export function persistUpdateChannel(channel: UpdateChannel): void {
  try {
    localStorage.setItem(UPDATE_CHANNEL_STORAGE_KEY, channel);
  } catch {
    // Ignore storage failures; the choice falls back to inference on next launch.
  }
}

export async function checkForUpdates(channel: UpdateChannel): Promise<UpdateCheckResult> {
  if (!hasTauri()) {
    return {
      currentVersion: "dev",
      available: false,
      error: localizedMessage("error.updatesUnavailable")
    };
  }
  return invoke<UpdateCheckResult>("check_for_updates", { channel });
}

export async function installUpdate(channel: UpdateChannel): Promise<void> {
  if (!hasTauri()) throw localizedMessage("error.updatesUnavailable");
  await invoke("install_update", { channel });
}
