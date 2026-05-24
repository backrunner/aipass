<script lang="ts">
  import { Dialog } from "bits-ui";
  import { AlertTriangle, Check, Copy, Download, ShieldCheck } from "lucide-svelte";

  import type { MaybePromise } from "../../types";
  import Button from "../shared/Button.svelte";

  export let recoveryKey = "";
  export let copied = "";
  export let onCopy: () => MaybePromise = () => {};
  export let onDownload: (() => MaybePromise) | undefined = undefined;
  export let onAcknowledge: () => MaybePromise = () => {};

  let confirmed = false;

  $: if (!recoveryKey) confirmed = false;
</script>

{#if recoveryKey}
  <Dialog.Root open={true} onOpenChange={(value) => { if (!value && confirmed) onAcknowledge(); }}>
    <Dialog.Portal>
      <Dialog.Overlay class="recovery-overlay" />
      <Dialog.Content class="recovery-content" interactOutsideBehavior="ignore" escapeKeydownBehavior="ignore">
        <header class="recovery-header">
          <div class="recovery-icon" aria-hidden="true"><ShieldCheck size={20} /></div>
          <div class="recovery-heading">
            <Dialog.Title class="recovery-title">Save your recovery key</Dialog.Title>
            <Dialog.Description class="recovery-sub">
              This is the only time we'll show this key. You'll need it if you ever forget your master password.
            </Dialog.Description>
          </div>
        </header>

        <div class="warning" role="alert">
          <AlertTriangle size={16} aria-hidden="true" />
          <div>
            <strong>Store it somewhere safe — offline.</strong>
            <span>If you lose both your master password and this key, your vault data is unrecoverable. AIPass has no copy of it.</span>
          </div>
        </div>

        <code class="recovery-key mono">{recoveryKey}</code>

        <ul class="checklist">
          <li>Use a password manager, encrypted note, or printed copy in a safe place.</li>
          <li>Never share it. Anyone with this key can reset your vault.</li>
          <li>It will not be shown again. You can rotate it later in Settings.</li>
        </ul>

        <label class="confirm">
          <input type="checkbox" bind:checked={confirmed} />
          <span>I have saved my recovery key in a safe place. I understand it cannot be retrieved later.</span>
        </label>

        <footer class="recovery-actions">
          <Button variant="secondary" on:click={() => onCopy()}>
            {#if copied === "recovery-key"}<Check size={14} />Copied{:else}<Copy size={14} />Copy key{/if}
          </Button>
          {#if onDownload}
            <Button variant="secondary" on:click={() => onDownload?.()}>
              <Download size={14} /> Download
            </Button>
          {/if}
          <span class="spacer"></span>
          <Button variant="primary" on:click={() => onAcknowledge()} disabled={!confirmed}>
            I saved it — continue
          </Button>
        </footer>
      </Dialog.Content>
    </Dialog.Portal>
  </Dialog.Root>
{/if}

<style lang="scss">
  :global(.recovery-overlay) {
    position: fixed;
    inset: 0;
    z-index: 60;
    background: rgba(13, 18, 32, 0.62);
    backdrop-filter: blur(4px);
  }

  :global(.recovery-content) {
    position: fixed;
    top: 50%;
    left: 50%;
    z-index: 61;
    transform: translate(-50%, -50%);
    width: min(520px, calc(100vw - 32px));
    padding: 24px;
    background: var(--surface);
    border: 1px solid var(--border);
    border-radius: var(--radius-lg);
    box-shadow: var(--shadow-modal);
    display: flex;
    flex-direction: column;
    gap: 16px;
  }

  .recovery-header {
    display: flex;
    align-items: flex-start;
    gap: 12px;
  }

  .recovery-icon {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 36px;
    height: 36px;
    border-radius: var(--radius);
    background: var(--accent-soft);
    color: var(--accent);
    flex-shrink: 0;
  }

  .recovery-heading {
    display: flex;
    flex-direction: column;
    gap: 4px;
  }

  :global(.recovery-title) {
    font-size: 16px;
    font-weight: 600;
    display: block;
  }

  :global(.recovery-sub) {
    color: var(--text-secondary);
    font-size: 13px;
    line-height: 1.45;
  }

  .warning {
    display: flex;
    align-items: flex-start;
    gap: 10px;
    padding: 10px 12px;
    border: 1px solid var(--danger);
    border-radius: var(--radius);
    background: var(--danger-soft);
    color: var(--danger);

    strong {
      display: block;
      font-size: 13px;
      font-weight: 600;
    }

    span {
      display: block;
      font-size: 12px;
      line-height: 1.45;
      color: var(--text-secondary);
      margin-top: 2px;
    }
  }

  .recovery-key {
    display: block;
    padding: 14px 16px;
    border: 1px dashed var(--border-strong);
    border-radius: var(--radius);
    background: var(--surface-2);
    color: var(--text);
    font-size: 13px;
    line-height: 1.6;
    word-break: break-all;
    white-space: normal;
    user-select: all;
  }

  .checklist {
    list-style: none;
    padding: 0;
    margin: 0;
    display: flex;
    flex-direction: column;
    gap: 6px;
    font-size: 12px;
    color: var(--text-secondary);
    line-height: 1.45;

    li {
      position: relative;
      padding-left: 16px;
    }

    li::before {
      content: "•";
      position: absolute;
      left: 4px;
      color: var(--text-tertiary);
    }
  }

  .confirm {
    display: flex;
    align-items: flex-start;
    gap: 10px;
    padding: 10px 12px;
    border: 1px solid var(--border);
    border-radius: var(--radius);
    background: var(--surface-2);
    font-size: 12px;
    line-height: 1.45;
    color: var(--text-secondary);
    cursor: pointer;

    input {
      margin-top: 2px;
      flex-shrink: 0;
      accent-color: var(--accent);
    }
  }

  .recovery-actions {
    display: flex;
    align-items: center;
    gap: 8px;
  }

  .spacer {
    flex: 1;
  }
</style>
