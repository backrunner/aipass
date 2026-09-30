# Magpie Provider Implementation Review

## Executive Summary

Magpie (https://github.com/yetone/magpie) is a local AI proxy aggregator written in Go that manages multiple AI provider accounts, handles quota management, and provides intelligent routing. This review analyzes their implementation to identify improvements for our aipass project.

## Key Findings

### 1. Provider Icon Management

**Magpie's Implementation:**
- **71+ built-in provider icons** stored as SVG/PNG assets in `internal/gui/assets/icons/`
- Icons named with `-color` suffix: `anthropic.svg`, `claude-color.svg`, `deepseek-color.svg`, `gemini-color.svg`, `openai-color.svg`
- Custom provider icons stored in `~/.config/magpie/icons/` with content-based hashing (SHA256)
- Icon storage format: `file:<hash>.<ext>` where hash is first 16 hex chars of SHA256
- Maximum icon size: 1MB
- Supported formats: PNG, JPEG, GIF, WebP, ICO, SVG
- Security: Icons can only be fetched from public HTTPS URLs with validation against SSRF attacks
- Automatic cleanup: Unused icons are pruned after 1 hour

**Our Current State:**
- `ProviderIcon` component exists but only shows first letter fallback for remote URLs
- No built-in icon library
- No content-based caching

**Recommendation:**
✅ **Add built-in provider icon library** with 20-30 major providers
✅ **Implement content-addressed icon storage** in desktop app data directory
✅ **Add icon upload/fetch capability** for custom providers

### 2. Provider-Specific Special Handling

**Magpie's Approach:**
Magpie implements provider-specific modules for vendors that need special handling:

#### Claude/Anthropic (`claude_warmup.go`, `claude_cloak.go`)
- **Window warming**: Automatically sends warmup requests when quota windows reset (5-hour and 7-day windows)
- **Billing metadata**: Adds Claude Code's billing/prefix/metadata identity to requests
- **Multi-account support**: Manages multiple Claude accounts with different OAuth tokens
- **Proxy per account**: Each Claude account can have its own proxy setting

#### Codex/ChatGPT (`codex_warmup.go`, `codex_daemon.go`, `codex_request.go`)
- **Rate limit management**: Tracks RPM/TPM limits and automatically switches between accounts
- **Background daemon**: Runs separate daemon process to maintain session state
- **Automatic warming**: Sends keepalive requests to prevent quota window expiration
- **Reset tracking**: Monitors when rate limits reset and schedules account switches

#### Gemini/Antigravity (`antigravity_models.go`, `gemini_*.go`)
- **Project-based routing**: Associates Google Cloud project IDs with accounts
- **Model tier handling**: Special handling for different Gemini model tiers (Flash, Pro, Ultra)
- **OAuth flow**: Custom OAuth implementation for Google accounts

#### Azure/Bedrock (`azure.go`, `bedrock_test.go`)
- **Deployment name mapping**: Maps Azure deployment names to standard model IDs
- **Regional endpoints**: Handles different AWS regions for Bedrock
- **ARN construction**: Builds proper Amazon Resource Names for model access

**Our Current State:**
- Generic provider handling without vendor-specific optimizations
- No quota window warming
- No automatic account switching on rate limits

**Recommendation:**
✅ **Implement provider-specific adapters** for major vendors (Claude, OpenAI, Gemini)
✅ **Add quota window tracking** and optional warming for Claude accounts
✅ **Build rate limit detection** and automatic fallback/switching logic

### 3. Balance/Quota Checking

**Magpie's Implementation:**
Magpie has extensive balance checking support across 15+ providers:

```go
// Supported balance endpoints:
- DeepSeek: https://api.deepseek.com/user/balance
- Moonshot/Kimi: https://api.moonshot.cn/v1/users/me/balance
- OpenRouter: https://openrouter.ai/api/v1/credits
- SiliconFlow: https://api.siliconflow.cn/v1/user/info
- StepFun: https://api.stepfun.com/v1/accounts
- Command Code: https://api.commandcode.ai/alpha/billing/credits
- AiHubMix: https://aihubmix.com/dashboard/billing/remain
- new-api relays: /api/usage/token or /api/user/self
```

**Custom Balance Path Support:**
Users can configure custom balance endpoints with JSONPath extraction:
```bash
magpie provider set my-relay \
  balance=https://relay.example.com/api/usage/token \
  balance.path='$data.total_available / 500000'
```

**Path Expression Features:**
- JSONPath selection: `$data.quota`, `credits.left`
- Arithmetic: `+`, `-`, `*`, `/` with parentheses
- Currency formatting: `$` or `¥` prefix, `%` suffix for percentages
- Multiple values: `5h: a.used / a.cap %; $credits.left`

**Our Current State:**
- Basic quota tracking from provider responses
- No standardized balance checking
- Limited provider-specific quota implementations

**Recommendation:**
✅ **Build balance checker framework** with provider-specific implementations
✅ **Add custom balance endpoint configuration** with JSONPath support
✅ **Implement quota visualization** in provider list/details

### 4. Account Management

**Magpie's Account Structure:**
```go
type Account struct {
  Agent string // provider id: claude, codex, gemini
  User  string // email or username
  Plan  string // subscription tier
  Stream bool  // stream-only backend
}
```

**Account Operations:**
```bash
magpie accounts add claude              # Add account with OAuth
magpie accounts switch claude user@example.com
magpie accounts forget claude user@example.com
magpie accounts project gemini user@example.com <project-id>
magpie accounts refresh --json          # Refresh all quotas
```

**Multi-Account Features:**
- **Active account tracking**: One active account per provider
- **Standby accounts**: Automatic failover when active account exhausted
- **Per-account proxy**: Each account can have separate proxy settings
- **Account-specific routing**: Route requests to specific accounts based on quota

**Our Current State:**
- Single credential per provider entry
- Multiple entries needed for multiple accounts of same provider
- No automatic account switching

**Recommendation:**
✅ **Add multi-credential support** to provider entries (primary + fallback keys)
✅ **Implement active/standby credential tracking**
✅ **Build credential rotation logic** based on quota/rate limits

### 5. Provider Select UI Enhancement

**Magpie's Provider List Display:**
```
  ● deepseek   deepseek-color.svg   api.deepseek.com   ● sk-***  12 models
  ● openai     openai-color.svg     api.openai.com     ● sk-***  25 models
  ○ anthropic  anthropic.svg        api.anthropic.com  ○ no key  8 models
```

**Icon Display Pattern:**
- Icon displayed before provider name
- Color-coded status indicator (● active, ○ inactive)
- Masked key display
- Model count summary

**Our Current Implementation Needs:**
Looking at our code, we need to enhance select components with provider icons.

**Recommendation:**
✅ **Add icon display to provider select options**
✅ **Show provider status indicators** (active, quota remaining, errors)
✅ **Display account identity** (email/username) in select options

### 6. Routing and Fallback

**Magpie's Routing Strategies:**

```go
type Provider struct {
  Routing  string   // "smart", "order", "rotate", "usage"
  Fallback []string // ["provider2/model", "provider3/model"]
  Affinity string   // "auto", "session", "turn", "off"
}
```

**Routing Modes:**
- **Smart** (default): Use first account while it has quota, then switch to account with most quota remaining
- **Order**: Try accounts in order, only use next when previous fails
- **Rotate**: Round-robin across accounts
- **Usage**: Always use least-used account first

**Affinity/Caching:**
- **Auto**: Keep conversation on same account to maximize prompt cache hits
- **Session**: Pin entire session to one account
- **Turn**: Allow switching between turns
- **Off**: No affinity, route purely by quota/usage

**Fallback Chain:**
```bash
magpie provider fallback primary-provider backup1/gpt-4 backup2/claude-opus-5
```

When primary provider fails (rate limited, quota exhausted, down), automatically try fallback chain.

**Our Current State:**
- No routing strategies
- No automatic fallback
- No cache affinity tracking

**Recommendation:**
✅ **Implement routing strategy configuration** per provider entry
✅ **Add fallback provider chain** with automatic failover
✅ **Track conversation affinity** to optimize cache usage

### 7. Proxy Configuration

**Magpie's Proxy Support:**

```go
type Provider struct {
  Proxy          string            // Global proxy for this provider
  AccountProxies map[string]string // Per-account proxy overrides
}
```

**Proxy Precedence:**
1. Account-specific proxy (AccountProxies[account])
2. Provider-level proxy
3. Global settings proxy
4. Environment variables (HTTP_PROXY, HTTPS_PROXY)
5. System proxy
6. "direct" keyword to bypass all proxies

**Use Case:**
Useful when:
- Different accounts need different proxies (work vs personal)
- Some providers blocked in certain regions
- Testing with local proxy like Charles/mitmproxy

**Our Current State:**
- Global proxy configuration in settings
- No per-provider or per-account proxy

**Recommendation:**
✅ **Add per-provider proxy configuration**
✅ **Support per-credential proxy override**
✅ **Add "direct" option to bypass global proxy**

### 8. Custom Headers

**Magpie's Header Support:**

```bash
magpie provider add "My Relay" \
  url=https://relay.example.com/v1 \
  key=sk-xxx \
  header.X-Org-Id=acme \
  header.X-Team=engineering

magpie provider set anthropic-ws2 \
  header.anthropic-workspace-id=wrkspc_...
```

**Features:**
- Headers applied to all requests (forwarding, tests, balance checks)
- Can override auth headers for custom gateways
- Support for workspace/organization routing
- Suggested headers per provider preset (HeaderHints)

**Our Current State:**
- Basic header support in provider configuration
- No preset-based header suggestions

**Recommendation:**
✅ **Add header hints/templates** for common providers
✅ **Validate required headers** for specific provider types
✅ **Show header examples** in provider add/edit UI

### 9. Provider Presets

**Magpie's Preset System:**

Magpie includes 50+ pre-configured provider presets grouped by:
- **Vendors** (20+): OpenAI, Anthropic, Google, DeepSeek, Moonshot, etc.
- **Relays** (15+): OpenRouter, AiHubMix, SiliconFlow, new-api, etc.
- **Local** (5+): Ollama, LM Studio, vLLM, LocalAI

**Preset Definition:**
```go
type PresetDef struct {
  ID          string
  Name        string
  Icon        string   // Built-in icon name
  Kind        Kind     // vendor/relay/local
  Chat        string   // Base URL
  Catalog     string   // models.dev catalog ID
  Website     string
  KeysURL     string   // Where to get API keys
  NoKey       bool     // Local servers
  Sponsored   bool     // Show first with tag
  Regions     []Region // Regional endpoints
  HeaderHints []string // Suggested headers
}
```

**Adding from Preset:**
```bash
magpie provider add deepseek sk-xxx
# Expands to full configuration with icon, URLs, catalog
```

**Our Current State:**
- Limited provider templates
- Manual configuration required for most providers

**Recommendation:**
✅ **Build comprehensive preset library** (30-50 providers)
✅ **Group presets by category** (official/relay/self-hosted)
✅ **Add regional endpoint support** for multi-region providers
✅ **Include sponsored/featured flag** for partnerships

### 10. Context Window Overrides

**Magpie's Feature:**

```bash
magpie provider set my-relay context=272k context.gpt-6=1m
```

Allows overriding reported context window sizes when:
- Vendor reports incorrect values
- Relay has different limits than upstream
- Want to constrain usage for cost reasons

**Use Cases:**
- Relay reports 200k but actually supports 1M
- Vendor reports 1M but you want to limit to 128k for cost control
- Model catalog outdated

**Our Current State:**
- Use vendor-reported context sizes
- No manual override capability

**Recommendation:**
✅ **Add context window override** per provider or model
✅ **Allow both global and per-model overrides**
✅ **Validate override against known limits**

## Implementation Priorities

Based on the Magpie review, here are actionable improvements prioritized by impact and complexity:

### Phase 1: Quick Wins (1-2 weeks)

#### 1.1 Provider Icon Library
**Impact:** High (Visual polish, professional appearance)
**Complexity:** Low

**Tasks:**
- [ ] Create `apps/desktop/src/assets/provider-icons/` directory
- [ ] Add SVG icons for 20-30 major providers:
  - Official: OpenAI, Anthropic, Google (Gemini), DeepSeek, Cohere, Mistral
  - Relays: OpenRouter, Cloudflare AI, Together AI
  - Open Source: Ollama, LM Studio, vLLM, LocalAI
- [ ] Update `ProviderIcon` component to check local assets before fallback
- [ ] Add icon mapping in provider presets

**Files to modify:**
- `apps/desktop/src/lib/components/providers/ProviderIcon.test.ts` (extend tests)
- `packages/ui/src/components/ProviderIcon.svelte` (add asset lookup)
- Create: `packages/schemas/src/provider-icons.ts` (icon mapping)

#### 1.2 Enhanced Provider Select with Icons
**Impact:** High (Better UX for provider selection)
**Complexity:** Low

**Tasks:**
- [ ] Find all select/dropdown components that show providers
- [ ] Add icon display before provider name in options
- [ ] Show account identity (email) when available
- [ ] Add status indicator dot (active/inactive/error)

**Pattern:**
```svelte
<Select.Item value={entry.id}>
  <div class="flex items-center gap-2">
    <ProviderIcon faviconUrl={entry.faviconUrl} kind={entry.providerKind} />
    <span>{entry.title}</span>
    {#if entry.accountIdentity}
      <span class="text-muted-foreground text-xs">({entry.accountIdentity})</span>
    {/if}
    <StatusDot status={getStatus(entry)} />
  </div>
</Select.Item>
```

#### 1.3 Balance Checker Framework
**Impact:** Medium (Enables quota monitoring)
**Complexity:** Medium

**Tasks:**
- [ ] Create `crates/provider-adapters/src/balance/` module
- [ ] Implement balance checkers for top 5 providers:
  - DeepSeek: `GET https://api.deepseek.com/user/balance`
  - Moonshot: `GET https://api.moonshot.cn/v1/users/me/balance`
  - OpenRouter: `GET https://openrouter.ai/api/v1/credits`
  - SiliconFlow: `GET https://api.siliconflow.cn/v1/user/info`
  - Together AI: `GET https://api.together.xyz/v1/me`
- [ ] Add balance refresh action to provider detail pane
- [ ] Store balance data in `quota` field with timestamp

**API:**
```rust
pub trait BalanceChecker: Send + Sync {
    async fn check_balance(&self, credential: &str) -> Result<Balance>;
}

pub struct Balance {
    pub amount: f64,
    pub currency: String,
    pub unit: String, // "credits", "tokens", "requests"
    pub checked_at: DateTime<Utc>,
}
```

### Phase 2: Account Management (2-3 weeks)

#### 2.1 Multi-Credential Support
**Impact:** High (Enables account switching and fallback)
**Complexity:** High

**Tasks:**
- [ ] Extend provider entry schema to support multiple credentials:
  ```typescript
  type ProviderEntry = {
    secretRefs: Array<{
      id: string;
      label: string;
      masked: string;
      fingerprint: string;
      active: boolean; // Currently in use
      standby: boolean; // Automatic failover
    }>;
  }
  ```
- [ ] Add credential management UI in provider detail pane
- [ ] Implement "Add Secondary Credential" action
- [ ] Add "Switch Active Credential" action
- [ ] Update gateway to use active credential from array

**Files to modify:**
- `packages/schemas/src/provider.ts` (extend schema)
- `apps/desktop/src/lib/components/providers/ProviderDetailPane.svelte` (UI)
- `crates/gateway/src/credentials.rs` (multi-credential logic)

#### 2.2 Routing Strategies
**Impact:** Medium (Better quota management)
**Complexity:** Medium

**Tasks:**
- [ ] Add routing configuration to provider entry:
  ```typescript
  type RoutingConfig = {
    strategy: "smart" | "order" | "rotate" | "usage";
    affinity: "auto" | "session" | "turn" | "off";
  }
  ```
- [ ] Implement smart routing (use best quota, fallback automatically)
- [ ] Track per-credential usage statistics
- [ ] Add routing strategy selector in provider settings

#### 2.3 Fallback Chain
**Impact:** Medium (Improved reliability)
**Complexity:** Medium

**Tasks:**
- [ ] Add fallback configuration to provider entry:
  ```typescript
  type ProviderEntry = {
    fallbackChain?: Array<{ providerId: string; model?: string }>;
  }
  ```
- [ ] Implement automatic failover on rate limit/quota exhausted
- [ ] Add retry logic with exponential backoff
- [ ] Show fallback status in request logs

### Phase 3: Provider-Specific Optimizations (3-4 weeks)

#### 3.1 Claude-Specific Features
**Impact:** High for Claude users
**Complexity:** High

**Tasks:**
- [ ] Implement quota window tracking (5-hour and 7-day windows)
- [ ] Add optional window warming:
  - Send warmup request when window resets
  - Configurable per account
  - Respect user preference (opt-in)
- [ ] Track prompt cache affinity
- [ ] Prefer same credential for conversation continuation

**Implementation:**
```rust
pub struct ClaudeAdapter {
    windows: Vec<QuotaWindow>,
    warmup_enabled: bool,
}

pub struct QuotaWindow {
    name: String,
    span: Duration,
    reset_at: Option<DateTime<Utc>>,
}

impl ClaudeAdapter {
    async fn maybe_warm_window(&self, window: &QuotaWindow) {
        if self.warmup_enabled && window.should_warm() {
            self.send_warmup_request().await;
        }
    }
}
```

#### 3.2 Rate Limit Detection
**Impact:** High (Better error handling)
**Complexity:** Medium

**Tasks:**
- [ ] Parse rate limit headers from responses:
  - `X-RateLimit-Remaining`
  - `X-RateLimit-Reset`
  - `Retry-After`
- [ ] Store rate limit state per credential
- [ ] Automatically switch credentials when rate limited
- [ ] Show rate limit status in UI

#### 3.3 Provider Adapters
**Impact:** Medium (Cleaner architecture)
**Complexity:** High

**Tasks:**
- [ ] Create adapter trait:
  ```rust
  pub trait ProviderAdapter: Send + Sync {
      fn transform_request(&self, req: Request) -> Result<Request>;
      fn transform_response(&self, res: Response) -> Result<Response>;
      fn check_balance(&self, credential: &str) -> BoxFuture<Result<Balance>>;
      fn track_quota(&self, usage: Usage) -> BoxFuture<Result<()>>;
  }
  ```
- [ ] Implement adapters for:
  - `ClaudeAdapter` (window tracking, warmup)
  - `OpenAIAdapter` (rate limit handling)
  - `GeminiAdapter` (project routing)
  - `GenericAdapter` (default behavior)
- [ ] Register adapters by provider ID or domain pattern

### Phase 4: Advanced Features (4+ weeks)

#### 4.1 Custom Balance Endpoints
**Impact:** Medium (Relay support)
**Complexity:** High

**Tasks:**
- [ ] Add balance configuration to provider:
  ```typescript
  type BalanceConfig = {
    url: string;
    path: string; // JSONPath: "$data.quota / 500000"
    token?: string; // Optional separate auth token
  }
  ```
- [ ] Implement JSONPath parser for extraction
- [ ] Support arithmetic expressions: `+`, `-`, `*`, `/`, `()`
- [ ] Support currency formatting: `$`, `¥`, `%`
- [ ] Support multiple values: `5h: a.used / a.cap %; $credits.left`

#### 4.2 Per-Provider Proxy
**Impact:** Low (Niche use case)
**Complexity:** Medium

**Tasks:**
- [ ] Add proxy configuration to provider entry
- [ ] Add per-credential proxy override
- [ ] Implement proxy precedence logic
- [ ] Support "direct" keyword to bypass global proxy

#### 4.3 Provider Preset Library
**Impact:** High (Easier onboarding)
**Complexity:** Medium

**Tasks:**
- [ ] Create provider preset definitions:
  ```typescript
  type ProviderPreset = {
    id: string;
    name: string;
    kind: "official" | "relay" | "self_hosted";
    icon: string;
    baseUrls: { chat?: string; anthropic?: string };
    catalog?: string;
    website?: string;
    keysUrl?: string;
    noKey?: boolean;
    regions?: Array<{ id: string; name: string; baseUrl: string }>;
    headerHints?: string[];
  }
  ```
- [ ] Build preset library with 30-50 providers
- [ ] Add "Add from Preset" flow in UI
- [ ] Auto-fill configuration when selecting preset

## Not Recommended from Magpie

Some Magpie features are specific to their CLI/daemon architecture and don't apply to our desktop app:

❌ **Background daemon** - We run as desktop app, not background service
❌ **CLI commands** - We have GUI, not CLI interface
❌ **Auto-start/system integration** - Out of scope for current phase
❌ **Import from other apps** - Nice to have, not priority

## Testing Strategy

For each phase:
1. Unit tests for core logic (balance checkers, routing, adapters)
2. Integration tests with mock providers
3. Manual testing with real provider accounts
4. Desktop UI validation at 960×640 (Tauri minimum)

## Success Metrics

- **Phase 1:** Provider icons visible in all select components
- **Phase 2:** Multi-credential switching working for 3 test accounts
- **Phase 3:** Automatic Claude window tracking with visual indicators
- **Phase 4:** Custom balance endpoint working for 2 relay providers

## Next Steps

1. Review and prioritize this document with team
2. Create GitHub issues for Phase 1 tasks
3. Assign Phase 1.1 (Provider Icons) as first implementation
4. Schedule design review for multi-credential UI (Phase 2.1)

## References

- Magpie Repository: https://github.com/yetone/magpie
- Key Files Reviewed:
  - `internal/provider/provider.go` - Core provider structure
  - `internal/provider/account.go` - Account management
  - `internal/provider/balance.go` - Balance checking
  - `internal/provider/icon.go` - Icon management
  - `internal/provider/claude_warmup.go` - Claude-specific logic
  - `internal/provider/presets.go` - Provider presets
  - `providers_cli.go` - CLI interface patterns
