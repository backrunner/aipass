import type { AuthScheme, InterfaceType } from "@aipass/schemas";

export type FormMode = "add" | "edit";

export type MaybePromise<T = void> = T | Promise<T>;

export type LocalePreference = "system" | "en" | "zh-CN";

export type MessageParams = Record<string, string | number | boolean | undefined>;

export type LocalizedMessage = {
  key: string;
  params?: MessageParams;
};

export type MessageValue = string | LocalizedMessage;

export type Draft = {
  title: string;
  domain: string;
  endpoint: string;
  consoleUrl: string;
  faviconUrl: string;
  providerId: string;
  /** Optional storage metadata carried by an AIPass provider deep link. */
  credentialKind?: "api" | "oauth";
  accountIdentity?: string;
  interfaceType: InterfaceType;
  authScheme: AuthScheme;
  apiKey: string;
  secretLabel: string;
  defaultModel: string;
  modelAlias: string;
  tag: string;
  header: string;
  quotaLabel: string;
  quotaLimit: string;
  quotaUsed: string;
  quotaRemaining: string;
  quotaResetAt: string;
  /** Gateway group this key belongs to. Stored on the key, not the entry. */
  gatewayGroup: string;
  /** Billing rate multiplier for the group, e.g. `1.5`. */
  gatewayRate: string;
  billingCurrency: string;
  billingUnitPrice: string;
  notes: string;
};
