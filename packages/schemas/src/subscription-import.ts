/** Metadata-only import contract. Credential payloads never cross this boundary. */
export interface SubscriptionImportSource {
  provider: string;
  root: string;
  selector?: string;
}
export interface SubscriptionImportInput {
  providerIds?: string[];
  sources?: SubscriptionImportSource[];
  retry?: { ticket: string; sourceIds: string[] } | null;
}
export type SubscriptionImportStatus = "imported" | "existing" | "updated" | "needs_login" | "not_found" | "failed" | "cancelled";
export interface SubscriptionImportResult {
  sourceId: string;
  source: SubscriptionImportSource;
  accountIdentity: string | null;
  status: SubscriptionImportStatus;
  entryId: string | null;
  errorCode: string | null;
  action: string | null;
}
export interface SubscriptionImportTask {
  ticket: string;
  phase: "discovering" | "importing" | "complete" | "cancelled";
  total: number;
  completed: number;
  results: SubscriptionImportResult[];
}
