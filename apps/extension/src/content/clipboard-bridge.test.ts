import assert from "node:assert/strict";
import { beforeEach, describe, it, vi } from "vitest";

function flushFrameworkScan() {
  return new Promise((resolve) => setTimeout(resolve, 120));
}

describe("clipboard bridge", () => {
  beforeEach(() => {
    vi.resetModules();
    document.title = "";
    document.body.innerHTML = "";
  });

  it("observes navigator.clipboard.writeText without changing the write result", async () => {
    const clipboard = {
      writeText: vi.fn().mockResolvedValue(undefined)
    };
    Object.defineProperty(navigator, "clipboard", {
      configurable: true,
      value: clipboard
    });
    const originalWriteText = clipboard.writeText;
    const observed: string[] = [];
    window.addEventListener("aipass.clipboardSecret", (event) => {
      observed.push((event as CustomEvent<{ text?: string }>).detail?.text ?? "");
    });

    // @ts-expect-error Dynamic import is used here only to execute the script in Vitest.
    await import("./clipboard-bridge");
    const result = await navigator.clipboard.writeText("sk-written1234567890");
    await new Promise((resolve) => setTimeout(resolve, 0));

    assert.notEqual(navigator.clipboard.writeText, originalWriteText);
    assert.equal(result, undefined);
    assert.equal(originalWriteText.mock.calls[0]?.[0], "sk-written1234567890");
    assert.deepEqual(observed, ["sk-written1234567890"]);
  });

  it("does not emit clipboard secrets when navigator.clipboard.writeText fails", async () => {
    const clipboard = {
      writeText: vi.fn().mockRejectedValue(new Error("denied"))
    };
    Object.defineProperty(navigator, "clipboard", {
      configurable: true,
      value: clipboard
    });
    const observed: string[] = [];
    window.addEventListener("aipass.clipboardSecret", (event) => {
      observed.push((event as CustomEvent<{ text?: string }>).detail?.text ?? "");
    });

    // @ts-expect-error Dynamic import is used here only to execute the script in Vitest.
    await import("./clipboard-bridge");
    await assert.rejects(() => navigator.clipboard.writeText("sk-denied1234567890"));
    await new Promise((resolve) => setTimeout(resolve, 0));

    assert.deepEqual(observed, []);
  });

  it("observes copy events without cancelling the page copy flow", async () => {
    const input = document.createElement("input");
    input.value = "sk-test1234567890";
    document.body.append(input);
    input.focus();
    input.setSelectionRange(0, input.value.length);
    const observed: string[] = [];
    window.addEventListener("aipass.clipboardSecret", (event) => {
      observed.push((event as CustomEvent<{ text?: string }>).detail?.text ?? "");
    });

    // @ts-expect-error Dynamic import is used here only to execute the script in Vitest.
    await import("./clipboard-bridge");
    const event = new Event("copy", { bubbles: true, cancelable: true });
    const copyAllowed = document.dispatchEvent(event);
    await new Promise((resolve) => setTimeout(resolve, 0));

    assert.equal(copyAllowed, true);
    assert.equal(event.defaultPrevented, false);
    assert.deepEqual(observed, ["sk-test1234567890"]);
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
