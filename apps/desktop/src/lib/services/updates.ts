import { invoke } from "@tauri-apps/api/core";

export type UpdateCheckResult = {
  currentVersion: string;
  available: boolean;
  latestVersion?: string;
  notes?: string;
  error?: string;
};

const hasTauri = () =>
  typeof window !== "undefined" &&
  Boolean((window as Window & { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__);

export async function checkForUpdates(): Promise<UpdateCheckResult> {
  if (!hasTauri()) {
    return {
      currentVersion: "dev",
      available: false,
      error: "Updates are unavailable in browser preview"
    };
  }
  return invoke<UpdateCheckResult>("check_for_updates");
}

export async function installUpdate(): Promise<void> {
  if (!hasTauri()) throw new Error("Updates are unavailable in browser preview");
  await invoke("install_update");
}
