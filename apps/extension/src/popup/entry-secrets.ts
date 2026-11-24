import type { Entry } from "./types";

export function entrySecrets(entry: Entry): NonNullable<Entry["secretRefs"]> {
  return entry.secretRefs ?? [
    { id: "primary", label: "primary", masked: entry.maskedSecret, fingerprint: entry.fingerprint },
  ];
}

export function matchesMaskedSecret(secret: string, maskedValue: string): boolean {
  const masked = maskedValue.trim();
  if (!masked || masked === "****") return true;
  if (masked.includes("...")) {
    const [head, tail] = masked.split("...");
    return (!head || secret.startsWith(head)) && (!tail || secret.endsWith(tail));
  }
  const suffix = masked.replace(/^••••\s*/, "").trim();
  return !suffix || secret.endsWith(suffix);
}
