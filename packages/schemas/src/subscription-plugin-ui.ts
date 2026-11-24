/** Inert presentation data for local subscription providers. */
export type PluginText = string | { en: string; "zh-CN"?: string };
export type PluginTone = "info" | "success" | "warning" | "danger";
export interface PluginPrompt {
  type: "text" | "secret" | "textarea" | "url" | "integer" | "select" | "radio" | "checkbox";
  key: string;
  message: PluginText;
  help?: PluginText;
  placeholder?: PluginText;
  options?: { label: PluginText; value: string }[];
  required?: boolean;
  default?: string;
  min?: number;
  max?: number;
  maxLength?: number;
  when?: { field: string; equals: string };
}
export interface PluginSection {
  title: PluginText;
  blocks: (
    | { type: "note"; text: PluginText; tone: PluginTone }
    | { type: "facts"; items: { label: PluginText; value: PluginText; copyable?: boolean }[] }
    | { type: "status"; label: PluginText; value: PluginText; tone: PluginTone }
    | { type: "progress"; label: PluginText; percent: number }
    | { type: "table"; columns: PluginText[]; rows: PluginText[][] }
  )[];
}

export function pluginText(text: PluginText, locale: string): string {
  if (typeof text === "string") return text;
  return locale === "zh-CN" ? text["zh-CN"] ?? text.en : text.en;
}

/** Evaluate conditions in manifest order and discard values from hidden controls. */
export function pluginValues(prompts: PluginPrompt[], inputs: Record<string, string>): Record<string, string> {
  const values: Record<string, string> = Object.create(null);
  for (const prompt of prompts) {
    if (prompt.when && values[prompt.when.field] !== prompt.when.equals) continue;
    const fallback = prompt.default ?? (prompt.type === "select" ? prompt.options?.[0]?.value : prompt.type === "checkbox" ? "false" : "");
    values[prompt.key] = Object.prototype.hasOwnProperty.call(inputs, prompt.key) ? inputs[prompt.key] : fallback ?? "";
  }
  return values;
}
