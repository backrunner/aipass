// Imported by the test copy of the unpacked extension before its real worker.
// This copy lacks nativeMessaging permission and cannot access the user's vault.
function entry(id, title) {
  return {
    id, title, domains: [], endpoints: [{ id: "api", kind: "api", url: "https://gateway.example.test/v1" }],
    interfaceType: "openai_compatible", authScheme: "bearer", maskedSecret: "•••• 1111",
    fingerprint: "first-fingerprint", secretRefs: [
      { id: "first", label: "Primary", masked: "•••• 1111", fingerprint: "first-fingerprint" },
      { id: "second", label: "Secondary", masked: "•••• 2222", fingerprint: "second-fingerprint", endpoint: "https://secondary.example.test/v1" },
    ],
  };
}

const fixture = globalThis.nativeFixture = {
  locked: false,
  entries: [entry("alpha", "Alpha Gateway"), entry("beta", "Beta Gateway")],
  calls: [],
  failures: {},
  hold: {},
  unexpected: [],
};

function respond(request) {
  const { type } = request;
  fixture.calls.push(request);
  if (fixture.failures[type]) throw Error(fixture.failures[type]);
  const status = { locked: fixture.locked, vaultNamespace: "fixture", language: { locale: "en", resolvedLocale: "en" } };
  switch (type) {
    case "ping": return status;
    case "ui.open_main": return { opened: true };
    case "session.unlock":
      if (request.password && request.password !== "test-only") throw Error("Unlock failed");
      fixture.locked = false;
      return { ...status, locked: false };
    case "entries.list": return { entries: fixture.entries, grants: [] };
    case "context.lookup": return { entries: [], grants: [] };
    case "entries.search": {
      const selected = fixture.entries[0];
      const ref = selected?.secretRefs.find((item) => item.fingerprint === request.query);
      const entries = fixture.entries.filter((item) => item.title.toLowerCase().includes(request.query.toLowerCase()));
      const grants = ref ? [{ id: "grant-" + ref.id, entryId: selected.id, secretId: ref.id, expiresAt: new Date(Date.now() + 120000).toISOString() }] : [];
      return { entries, grants };
    }
    case "provider.usageProbe": return { ok: true, quota: { label: "Quota fixture", remaining: "777" } };
    case "provider.usageApply": {
      const item = fixture.entries.find((entry) => entry.id === request.entry_id);
      if (item) item.quota = request.quota;
      return { entryId: request.entry_id };
    }
    case "provider.faviconBackfill": return { entries: [], checked: 0, updated: 0, skipped: 0, errors: [] };
    case "settings.isOriginIgnored": return { ignored: false };
    case "settings.ignoreOrigin": return { ignoredOrigins: [request.origin] };
    case "provider.add": {
      const added = entry("added", request.title);
      fixture.entries.push(added);
      return { entryId: added.id };
    }
    case "provider.update": {
      const item = fixture.entries.find((item) => item.id === request.entry_id);
      item.title = request.title;
      return { entryId: item.id };
    }
    case "provider.delete":
      fixture.entries = fixture.entries.filter((item) => item.id !== request.entry_id);
      return { entryId: request.entry_id, deleted: true };
    case "secret.fill":
      return { secret: fixture.returnedSecret ?? (request.field_id === "second" ? "fixture-2222" : "fixture-1111") };
    case "secret.previewDetected":
      return { title: request.title, maskedSecret: "•••• TEST", fingerprint: "fixture-fingerprint", tags: [], isSaved: false };
    case "secret.saveDetected":
      return { entryId: "saved-draft" };
    case "secret.add":
    case "secret.update":
    case "secret.remove": {
      const item = fixture.entries.find((item) => item.id === request.entry_id);
      if (type === "secret.remove") item.secretRefs = item.secretRefs.filter((ref) => ref.id !== request.secret_id);
      else if (type === "secret.update") Object.assign(item.secretRefs.find((ref) => ref.id === request.secret_id), {label: request.label}, request.metadata);
      else item.secretRefs.push({ id: "added-key", label: request.label, masked: "•••• 3333", fingerprint: "third-fingerprint", ...request.metadata });
      return { entryId: item.id, secretId: request.secret_id ?? "added-key", entry: item };
    }
    default:
      fixture.unexpected.push(type);
      throw Error(`Unexpected native request: ${type}`);
  }
}

chrome.runtime.sendNativeMessage = (_host, request, callback) => {
  let response;
  try {
    response = { ok: true, id: request.id, data: respond(request) };
  } catch (error) {
    response = { ok: false, id: request.id, error: error.message };
  }
  Promise.resolve(fixture.hold[request.type]).then(() => callback(structuredClone(response)));
};
