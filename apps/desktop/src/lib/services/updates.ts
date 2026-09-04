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

function clearStoredUpdateChannel(): void {
  try {
    localStorage.removeItem(UPDATE_CHANNEL_STORAGE_KEY);
  } catch {
    // Ignore storage failures; the inferred channel still wins for this run.
  }
}

export function resolveUpdateChannel(version: string): UpdateChannel {
  if (!version) return getStoredUpdateChannel() ?? "official";
  const inferred = inferUpdateChannel(version);
  const stored = getStoredUpdateChannel();
  // A manual override only survives within the same version family; once the
  // build switches between beta and official, the stored value must not keep
  // polling the wrong feed.
  if (stored && stored !== inferred) clearStoredUpdateChannel();
  return inferred;
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

export async function downloadUpdate(channel: UpdateChannel): Promise<string> {
  if (!hasTauri()) throw localizedMessage("error.updatesUnavailable");
  return invoke<string>("download_update", { channel });
}

export async function installPendingUpdate(channel: UpdateChannel): Promise<boolean> {
  if (!hasTauri()) return false;
  return invoke<boolean>("install_pending_update", { channel });
}

export async function clearPendingUpdate(): Promise<void> {
  if (!hasTauri()) return;
  await invoke("clear_pending_update");
}

export async function installUpdate(channel: UpdateChannel): Promise<void> {
  if (!hasTauri()) throw localizedMessage("error.updatesUnavailable");
  await invoke("install_update", { channel });
}
