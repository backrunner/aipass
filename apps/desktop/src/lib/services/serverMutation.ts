/** Owns UI mutation feedback; persistence and authorization remain in the Agent. */
export interface ServerMutationHost {
  busy: string;
  readonly locked: boolean;
  readonly revision: number;
  begin(): void;
  finish(): void;
  clearError(): void;
  reportError(error: string, operation: string): void;
}

export async function runServerMutation(
  host: ServerMutationHost,
  operation: string,
  action: (current: () => boolean) => Promise<boolean>
): Promise<boolean> {
  if (host.busy || host.locked) return false;
  host.busy = operation;
  host.begin();
  const revision = host.revision;
  const current = () => !host.locked && revision === host.revision;
  host.clearError();
  try {
    return await action(current);
  } catch (error) {
    if (current()) host.reportError(String(error), operation);
    return false;
  } finally {
    host.finish();
    host.busy = "";
  }
}
