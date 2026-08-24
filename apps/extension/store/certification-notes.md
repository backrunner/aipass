# Microsoft Edge certification notes

## Test environment

This submission is the AIPass Manifest V3 extension. It is designed to pair with the AIPass desktop app and its local Native Messaging host. No web account or remote test credentials are required.

Before submitting, replace `{{EDGE_EXTENSION_ID}}` below with the Edge ID shown by Partner Center after the package is uploaded:

```bash
aipass native-host install --browser edge --extension-id {{EDGE_EXTENSION_ID}}
```

Launch the AIPass desktop app, create a local test vault with a non-production password, and add a test provider entry. The test vault may use a non-production key or a fake value; do not use a live production credential.

## Certification flow

1. Install the submitted package in Microsoft Edge.
2. Start AIPass and unlock the test vault in the desktop app.
3. Open the extension popup on a provider console. The popup should show `Connected` and list matching test entries.
4. Select the test entry and choose Fill. Confirm that the value is inserted only after the user action.
5. Lock the AIPass vault and reopen the popup. The extension must not list, reveal, or fill a credential while locked.
6. On a supported provider key page, create or paste a non-production test value. Review the detected draft; saving requires an explicit confirmation.
7. Use Ignore site for the current origin, reload it, and confirm that lookup and detection prompts stop for that origin.
8. Disconnect or quit the AIPass desktop app. The popup should show a clear disconnected state and must not fail the page or expose a secret.

## Important implementation notes

- The extension does not collect, sell, or transmit credentials or browsing activity to AIPass servers.
- API keys are not persisted in extension storage. Only session-scoped display metadata and UI preferences are cached.
- The extension does not use remote code. All executable code is inside the uploaded ZIP.
- The `nativeMessaging` permission is required for the local AIPass agent. The `<all_urls>` content-script scope is required for provider-console detection and is disabled per origin when the user chooses Ignore site.
- The store package is independent of the Chrome Web Store. The same MV3 build supports Microsoft Edge; native host installation uses the Edge browser target.
