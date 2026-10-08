# Phase 2: Account Management - ✅ COMPLETED

## What Was Implemented

### 1. CredentialManager Component
Created comprehensive credential management UI at `apps/desktop/src/lib/components/providers/CredentialManager.svelte`

**Features:**
- Visual credential list with priority ordering
- Primary credential badge with star icon
- Drag-to-reorder functionality (up/down arrows)
- Set primary credential action
- Add/Remove credential controls
- Masked key display with metadata
- Group, endpoint, and model display per credential
- Priority numbering (1, 2, 3...)

**Key Operations:**
```typescript
- onAddCredential() - Add new credential to provider
- onRemoveCredential(secretId) - Remove credential (requires 2+ credentials)
- onSetPrimary(secretId) - Promote credential to primary
- onReorderCredential(secretId, "up" | "down") - Change priority order
```

**Visual Design:**
```
┌─────────────────────────────────────────────┐
│ [KeyIcon] primary      ⭐ Primary           │
│ Masked: sk-proj-***abc123                   │
│ Group: default                              │
│ Priority: 1                                 │
├─────────────────────────────────────────────┤
│ [KeyIcon] backup-1     [Set Primary] ↑ ↓ 🗑 │
│ Masked: sk-proj-***xyz789                   │
│ Endpoint: https://api.custom.com            │
│ Priority: 2                                 │
└─────────────────────────────────────────────┘
```

### 2. RoutingStrategyConfig Component
Created routing strategy configuration at `apps/desktop/src/lib/components/providers/RoutingStrategyConfig.svelte`

**Routing Strategies:**

1. **Smart** (Default)
   - Use first credential while it has quota
   - Switch to credential with most quota remaining
   - Automatic failover on exhaustion

2. **Order**
   - Try credentials in priority order
   - Only use next when previous fails
   - Strict fallback chain

3. **Rotate**
   - Round-robin across all credentials
   - Even distribution of requests
   - Load balancing

4. **Usage**
   - Always use least-used credential first
   - Tracks request count per credential
   - Maximizes quota utilization

**Affinity Modes:**

1. **Auto** (Default)
   - Keep conversation on same credential
   - Maximize prompt cache hits
   - Switch only when necessary

2. **Session**
   - Pin entire session to one credential
   - No switching unless credential fails
   - Maximum cache efficiency

3. **Turn**
   - Allow switching between turns
   - Balance between caching and quota
   - Flexible routing

4. **Off**
   - No affinity tracking
   - Route purely by quota/usage
   - Ignore cache optimization

**Fallback Chain:**
- Configure backup providers
- Automatic failover when primary exhausted
- Cross-provider redundancy

## Usage Examples

### Using CredentialManager in Provider Settings

```svelte
<script>
  import CredentialManager from "./CredentialManager.svelte";
  
  export let entry: ProviderEntry;
  
  async function handleAddCredential() {
    // Open add credential dialog
  }
  
  async function handleRemoveCredential(secretId: string) {
    // Remove credential from entry
    await removeCredential(entry.id, secretId);
  }
  
  async function handleSetPrimary(secretId: string) {
    // Update credential label to "primary"
    await setPrimaryCredential(entry.id, secretId);
  }
  
  async function handleReorder(secretId: string, direction: "up" | "down") {
    // Reorder credentials in array
    await reorderCredential(entry.id, secretId, direction);
  }
</script>

<CredentialManager
  {entry}
  onAddCredential={handleAddCredential}
  onRemoveCredential={handleRemoveCredential}
  onSetPrimary={handleSetPrimary}
  onReorderCredential={handleReorder}
/>
```

### Using RoutingStrategyConfig

```svelte
<script>
  import RoutingStrategyConfig from "./RoutingStrategyConfig.svelte";
  import type { RoutingStrategy, AffinityMode } from "./RoutingStrategyConfig.svelte";
  
  let strategy: RoutingStrategy = "smart";
  let affinity: AffinityMode = "auto";
  let enableFallback = false;
  let fallbackProviders: string[] = [];
  
  async function handleStrategyChange(newStrategy: RoutingStrategy) {
    strategy = newStrategy;
    await updateProviderConfig(entry.id, { strategy });
  }
  
  async function handleAffinityChange(newAffinity: AffinityMode) {
    affinity = newAffinity;
    await updateProviderConfig(entry.id, { affinity });
  }
</script>

<RoutingStrategyConfig
  {strategy}
  {affinity}
  {enableFallback}
  {fallbackProviders}
  onStrategyChange={handleStrategyChange}
  onAffinityChange={handleAffinityChange}
  onFallbackToggle={(enabled) => enableFallback = enabled}
/>
```

## Integration with Existing Infrastructure

### Primary Credential Pattern (Already Exists)
```typescript
// From aipass-provider-registry/src/lib.rs
pub const PRIMARY_SECRET_LABEL: &str = "primary";

pub fn primary_secret_ref(refs: &[SecretRef]) -> Option<&SecretRef> {
    refs.iter()
        .find(|secret| secret.label == PRIMARY_SECRET_LABEL)
        .or_else(|| refs.first())
}
```

The UI leverages this existing backend pattern:
- Primary credential has `label: "primary"`
- Falls back to first credential if no primary set
- CredentialManager visualizes this with star badge

### Multi-Credential Support (Already Exists)
```typescript
interface ProviderEntry {
  secretRefs: SecretRef[];  // Already supports multiple credentials
}
```

## Routing Strategy Logic (Backend Implementation Needed)

The UI provides configuration; backend implementation would follow this pattern:

```rust
pub struct RoutingConfig {
    pub strategy: RoutingStrategy,
    pub affinity: AffinityMode,
    pub fallback_providers: Vec<String>,
}

pub enum RoutingStrategy {
    Smart,    // Use credential with most quota
    Order,    // Try in priority order
    Rotate,   // Round-robin
    Usage,    // Least-used first
}

pub enum AffinityMode {
    Auto,     // Keep conversation on same credential
    Session,  // Pin entire session
    Turn,     // Allow per-turn switching
    Off,      // No affinity
}
```

**Smart Strategy Implementation:**
```rust
fn select_credential_smart(
    credentials: &[SecretRef],
    quota_cache: &HashMap<String, QuotaInfo>
) -> Option<&SecretRef> {
    // 1. Find credential with quota remaining
    // 2. If current has quota, keep using it (affinity)
    // 3. Otherwise, switch to credential with most remaining
    credentials.iter()
        .filter(|c| has_quota(c, quota_cache))
        .max_by_key(|c| quota_remaining(c, quota_cache))
}
```

## Files Created

- ✅ Created: `apps/desktop/src/lib/components/providers/CredentialManager.svelte`
- ✅ Created: `apps/desktop/src/lib/components/providers/RoutingStrategyConfig.svelte`

## Benefits

1. **Multi-Account Support**: Manage multiple credentials per provider
2. **Intelligent Routing**: Smart quota-based credential selection
3. **Cache Optimization**: Affinity modes maximize prompt cache hits
4. **High Availability**: Automatic failover with fallback chains
5. **Visual Priority**: Clear indication of credential order and primary
6. **Flexible Strategies**: Four routing modes for different use cases

## Integration Points

### Required Backend Work (Future)
To fully implement Phase 2, the backend needs:

1. **Routing Engine**
   - Credential selection based on strategy
   - Quota tracking per credential
   - Usage tracking per credential
   - Affinity session management

2. **Quota Monitoring**
   - Track remaining quota per credential
   - Detect exhaustion and trigger failover
   - Update quota from provider responses

3. **Failover Logic**
   - Automatic credential switching on failure
   - Cross-provider fallback chain
   - Rate limit detection and backoff

4. **Configuration Storage**
   - Persist routing strategy per provider
   - Store affinity preference
   - Save fallback chain

### Frontend Integration
Components can be integrated into:
- ProviderDetailPane (credential management tab)
- Provider settings dialog
- Advanced configuration section

## Next Steps - Phase 3

With credential management UI complete, Phase 3 can focus on:

**Provider-Specific Optimizations:**
- Claude quota window tracking and warmup
- Rate limit detection and automatic switching
- Provider-specific balance check optimizations
- Automatic health status monitoring

**Backend Implementation:**
- Routing engine with strategy execution
- Quota/usage tracking per credential
- Automatic failover logic
- Session affinity tracking

## Related Phases

- **Phase 1.1**: Provider icons ✅
- **Phase 1.2**: Status indicators ✅
- **Phase 1.3**: Quota/subscription display ✅
- **Phase 2**: Account management ✅ (UI complete)
- **Phase 3**: Provider-specific optimizations (next)

The UI foundation for advanced account management is now complete. Backend implementation can proceed independently while the UI provides immediate value for manual credential management.
