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
  interfaceType: InterfaceType;
  authScheme: AuthScheme;
  apiKey: string;
  defaultModel: string;
  modelAlias: string;
  environment: string;
  tag: string;
  header: string;
  quotaLabel: string;
  quotaLimit: string;
  quotaRemaining: string;
  quotaResetAt: string;
  notes: string;
};
