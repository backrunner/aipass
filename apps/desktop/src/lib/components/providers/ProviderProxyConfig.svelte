<script lang="ts">
  import { Field, SwitchField } from "@aipass/ui";
  import { AlertCircle, Globe } from "lucide-svelte";

  import { t } from "../../stores/i18n";
  import type { MaybePromise } from "../../types";

  interface ProxyConfig {
    enabled: boolean;
    proxyUrl?: string;
    proxyAuth?: {
      username: string;
      password: string;
    };
    bypassDomains?: string[];
    useSystemProxy: boolean;
  }

  export let config: ProxyConfig;
  export let onConfigChange: (config: ProxyConfig) => MaybePromise = () => {};

  // Initialize proxyAuth if undefined to prevent undefined access bugs
  $: if (config.enabled && !config.useSystemProxy && !config.proxyAuth) {
    config.proxyAuth = { username: "", password: "" };
  }

  let bypassDomainsText = config.bypassDomains?.join(", ") || "";

  function handleChange(updates: Partial<ProxyConfig>) {
    config = { ...config, ...updates };
    onConfigChange(config);
  }

  function handleBypassDomainsChange() {
    const domains = bypassDomainsText
      .split(",")
      .map((d) => d.trim())
      .filter((d) => d.length > 0);
    handleChange({ bypassDomains: domains });
  }
</script>

<div class="proxy-config">
  <div class="config-header">
    <Globe size={16} />
    <h3>{$t("proxy.configuration")}</h3>
  </div>

  <SwitchField
    bind:checked={config.enabled}
    label={$t("proxy.enableProxy")}
    description={$t("proxy.enableProxyDesc")}
    onCheckedChange={(checked) => handleChange({ enabled: checked })}
  />

  {#if config.enabled}
    <div class="proxy-options">
      <SwitchField
        bind:checked={config.useSystemProxy}
        label={$t("proxy.useSystemProxy")}
        description={$t("proxy.useSystemProxyDesc")}
        onCheckedChange={(checked) => handleChange({ useSystemProxy: checked })}
      />

      {#if !config.useSystemProxy}
        <Field label={$t("proxy.proxyUrl")}>
          <input
            type="text"
            bind:value={config.proxyUrl}
            placeholder="http://proxy.example.com:8080"
            on:change={() => handleChange({ proxyUrl: config.proxyUrl })}
          />
          <span class="field-hint">{$t("proxy.proxyUrlHint")}</span>
        </Field>

        <div class="proxy-auth-section">
          <h4>{$t("proxy.authentication")}</h4>
          <p class="section-desc">{$t("proxy.authenticationDesc")}</p>

          <div class="auth-fields">
            <Field label={$t("proxy.username")}>
              <input
                type="text"
                bind:value={config.proxyAuth!.username}
                placeholder={$t("proxy.usernamePlaceholder")}
                on:change={() => handleChange({ proxyAuth: config.proxyAuth })}
              />
            </Field>

            <Field label={$t("proxy.password")}>
              <input
                type="password"
                bind:value={config.proxyAuth!.password}
                placeholder={$t("proxy.passwordPlaceholder")}
                on:change={() => handleChange({ proxyAuth: config.proxyAuth })}
              />
            </Field>
          </div>
        </div>

        <Field label={$t("proxy.bypassDomains")}>
          <input
            type="text"
            bind:value={bypassDomainsText}
            placeholder="localhost, 127.0.0.1, *.internal.com"
            on:blur={handleBypassDomainsChange}
          />
          <span class="field-hint">{$t("proxy.bypassDomainsHint")}</span>
        </Field>
      {/if}
    </div>
  {/if}

  <div class="proxy-info">
    <AlertCircle size={14} />
    <div class="info-content">
      <strong>{$t("proxy.infoTitle")}</strong>
      <p>{$t("proxy.infoDesc")}</p>
    </div>
  </div>
</div>

<style lang="scss">
  .proxy-config {
    display: flex;
    flex-direction: column;
    gap: 16px;
  }

  .config-header {
    display: flex;
    align-items: center;
    gap: 8px;
    color: var(--text-secondary);

    h3 {
      margin: 0;
      font-size: 14px;
      font-weight: 650;
      color: var(--text);
    }
  }

  .proxy-options {
    display: flex;
    flex-direction: column;
    gap: 14px;
    padding: 14px;
    border: 1px solid var(--border);
    border-radius: var(--radius);
    background: var(--surface-2);
  }

  .field-hint {
    display: block;
    margin-top: 4px;
    font-size: 10px;
    color: var(--text-tertiary);
    line-height: 1.4;
  }

  .proxy-auth-section {
    display: flex;
    flex-direction: column;
    gap: 10px;
    padding: 12px;
    border-radius: var(--radius-sm);
    background: var(--surface-raised);

    h4 {
      margin: 0;
      font-size: 12px;
      font-weight: 650;
      color: var(--text);
    }

    .section-desc {
      margin: 0;
      font-size: 11px;
      color: var(--text-tertiary);
      line-height: 1.5;
    }
  }

  .auth-fields {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: 10px;
  }

  .proxy-info {
    display: flex;
    align-items: flex-start;
    gap: 8px;
    padding: 10px 12px;
    border-radius: var(--radius-sm);
    background: var(--surface-2);
    color: var(--text-secondary);
  }

  .info-content {
    display: flex;
    flex-direction: column;
    gap: 4px;
    flex: 1;

    strong {
      font-size: 11px;
      font-weight: 650;
      color: var(--text);
    }

    p {
      margin: 0;
      font-size: 10px;
      color: var(--text-tertiary);
      line-height: 1.5;
    }
  }

  @media (max-width: 1000px) {
    .auth-fields {
      grid-template-columns: 1fr;
    }
  }
</style>
