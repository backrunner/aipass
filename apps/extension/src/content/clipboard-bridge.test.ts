import assert from "node:assert/strict";
import { beforeEach, describe, it, vi } from "vitest";

function flushFrameworkScan() {
  return new Promise((resolve) => setTimeout(resolve, 120));
}

describe("clipboard bridge", () => {
  beforeEach(() => {
    vi.resetModules();
    delete (window as Window & { __AIPASS_CLIPBOARD_BRIDGE__?: boolean }).__AIPASS_CLIPBOARD_BRIDGE__;
    document.title = "";
    document.body.innerHTML = "";
  });

  it("extracts Sub2API custom keys from Vue state when the table only renders masked text", async () => {
    document.title = "API Keys - Relay Site";
    document.body.innerHTML = `
      <div id="app">
        <h1>API Keys</h1>
        <table>
          <tbody>
            <tr data-row-id="1">
              <td>Production</td>
              <td><code>produ...cdef</code><button>Copy</button></td>
            </tr>
          </tbody>
        </table>
      </div>`;
    const app = document.getElementById("app") as HTMLElement & {
      __vueParentComponent?: unknown;
    };
    app.__vueParentComponent = {
      setupState: {
        apiKeys: {
          value: [
            {
              name: "Production",
              key: "productA_key_1234567890abcdef"
            }
          ]
        }
      }
    };

    const emitted: string[] = [];
    window.addEventListener(
      "aipass.clipboardSecret",
      (event) => {
        emitted.push((event as CustomEvent<{ text?: string }>).detail?.text ?? "");
      },
      { once: true }
    );

    // clipboard-bridge is intentionally a classic content script without exports.
    // @ts-expect-error Dynamic import is used here only to execute the script in Vitest.
    await import("./clipboard-bridge");
    window.dispatchEvent(new CustomEvent("aipass.frameworkSecretScan", { detail: { enabled: true } }));
    await flushFrameworkScan();

    assert.deepEqual(emitted, ["productA_key_1234567890abcdef"]);
  });
});
