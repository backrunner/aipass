# CLI Proxy Features

This document describes the comprehensive CLI and MCP support for local proxy operations in AiPass.

## CLI Commands

All proxy operations are available through the `aipass proxy` command.

### Server Management

```bash
# Check proxy server status
aipass proxy status

# Start the proxy server
aipass proxy start

# Stop the proxy server
aipass proxy stop
```

### Configuration Management

```bash
# Get current proxy configuration
aipass proxy config-get

# Set proxy configuration from file
aipass proxy config-set --file <path>
```

### Route Management

```bash
# List all routes
aipass proxy route-list

# Create a new route
aipass proxy route-create \
  --name "My Route" \
  --provider-id <uuid> \
  --secret-id <secret-id> \
  --inbound-protocol openai-chat-completions \
  --upstream-protocol anthropic-messages \
  --conversion-enabled true

# Delete a route
aipass proxy route-delete <route-id>

# Enable/disable a route
aipass proxy route-set-enabled <route-id> --enabled true
aipass proxy route-set-enabled <route-id> --enabled false

# Select a route as active
aipass proxy route-select <route-id>

# Rotate route token
aipass proxy token-rotate <route-id>

# Apply route to agent tool (codex, claude-code, cursor, etc.)
aipass proxy route-apply <tool> <route-id>
```

### Group Management

```bash
# List all groups
aipass proxy group-list

# Switch to a specific group (disables all other groups)
aipass proxy group-switch <route-id> <group-name>

# Enable targets in a group
aipass proxy group-enable <route-id> <group-name>

# Disable targets in a group
aipass proxy group-disable <route-id> <group-name>
```

### Target Management

```bash
# List targets in a route
aipass proxy target-list <route-id>

# Add a target to a route
aipass proxy target-add <route-id> \
  --provider-id <uuid> \
  --secret-id <secret-id> \
  --group <group-name> \
  --priority 100 \
  --weight 1

# Remove a target from a route
aipass proxy target-remove <route-id> <target-id>

# Enable/disable a target
aipass proxy target-enable <route-id> <target-id>
aipass proxy target-disable <route-id> <target-id>

# Set target priority
aipass proxy target-set-priority <route-id> <target-id> --priority 50

# Set target weight
aipass proxy target-set-weight <route-id> <target-id> --weight 2
```

### Provider Management

```bash
# Update provider settings (WebSocket support, concurrency limits)
aipass proxy provider-update <provider-id> \
  --prefer-websocket true \
  --max-concurrent-requests 10
```

### Logs and Usage

```bash
# View proxy logs
aipass proxy logs --limit 100

# View usage statistics
aipass proxy usage --days 7

# Clear usage data
aipass proxy usage-clear
```

### Vault Management

Note: The `login` command has been renamed to `unlock`:

```bash
# Unlock the vault
aipass unlock

# Change vault password
aipass vault change-password --new-password <password>

# View vault status
aipass vault status
```

## MCP Server

The AiPass Proxy MCP server provides the same functionality through the Model Context Protocol, enabling AI assistants to manage proxy operations.

### Setup

Add to your MCP configuration (e.g., `~/.claude/mcp.json`):

```json
{
  "mcpServers": {
    "aipass-proxy": {
      "command": "cargo",
      "args": ["run", "--package", "aipass-mcp-proxy"],
      "description": "AiPass Proxy MCP Server"
    }
  }
}
```

Or use the compiled binary directly:

```json
{
  "mcpServers": {
    "aipass-proxy": {
      "command": "/path/to/aipass-mcp-proxy"
    }
  }
}
```

### Available MCP Tools

All CLI commands are available as MCP tools with the `proxy_` prefix:

- `proxy_status` - Get server status
- `proxy_start` - Start server
- `proxy_stop` - Stop server
- `proxy_restart` - Restart server
- `proxy_list_routes` - List all routes
- `proxy_create_route` - Create a new route
- `proxy_update_route` - Update route settings
- `proxy_delete_route` - Delete a route
- `proxy_enable_route` - Enable a route
- `proxy_disable_route` - Disable a route
- `proxy_select_route` - Select active route
- `proxy_rotate_token` - Rotate route token
- `proxy_list_targets` - List targets in a route
- `proxy_enable_target` - Enable a target
- `proxy_disable_target` - Disable a target
- `proxy_set_target_priority` - Set target priority
- `proxy_set_target_weight` - Set target weight
- `proxy_update_priorities` - Batch update priorities
- `proxy_get_logs` - Get proxy logs
- `proxy_get_usage` - Get usage statistics
- `proxy_clear_usage` - Clear usage data
- `proxy_apply_to_tool` - Apply proxy to agent tool
- `proxy_switch_group` - Switch to a group
- `proxy_add_target` - Add a new target
- `proxy_remove_target` - Remove a target
- `provider_update` - Update provider settings

## Desktop Integration

The desktop application automatically installs and makes the CLI globally accessible on first launch. The CLI binary is installed to:

- **macOS**: `~/.local/bin/aipass`
- **Windows**: `%LOCALAPPDATA%\aipass\bin\aipass.exe`
- **Linux**: `~/.local/bin/aipass`

The installation directory is automatically added to the system PATH.

## JSON Output

All CLI commands support `--json` flag for machine-readable output:

```bash
aipass --json proxy status
aipass --json proxy route-list
aipass --json proxy target-list <route-id>
```

## Examples

### Example 1: Create and Configure a Route

```bash
# Create a route
aipass proxy route-create \
  --name "Production Route" \
  --provider-id abc123... \
  --secret-id my-api-key

# Add additional targets with groups
aipass proxy target-add <route-id> \
  --provider-id def456... \
  --secret-id backup-key \
  --group backup \
  --priority 50

# Apply to Claude Code
aipass proxy route-apply claude-code <route-id>
```

### Example 2: Group Management

```bash
# Switch to backup group
aipass proxy group-switch <route-id> backup

# Switch back to primary
aipass proxy group-switch <route-id> primary
```

### Example 3: Provider WebSocket Support

```bash
# Enable WebSocket for a provider
aipass proxy provider-update <provider-id> \
  --prefer-websocket true
```

## Architecture

The implementation consists of:

1. **aipass-cli** - Command-line interface with all proxy operations
2. **aipass-mcp-proxy** - MCP server exposing proxy operations
3. **aipass-agent** - Background agent managing the proxy server
4. **aipass-desktop** - Desktop app with automatic CLI installation

All components share the same underlying agent protocol, ensuring consistency across interfaces.
