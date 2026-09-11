# 代理操作 CLI 和 MCP 实现文档

## 概述

本文档描述了 AIPass 中本地代理操作的 CLI 命令和 MCP 服务器实现。

## 功能清单

### ✅ 已完成功能

#### 1. CLI 命令 (`aipass proxy`)

##### 服务器管理
- `aipass proxy status` - 查看代理服务器状态
- `aipass proxy start` - 启动代理服务器
- `aipass proxy stop` - 停止代理服务器
- `aipass proxy restart` - 重启代理服务器

##### 路由（分组）管理
- `aipass proxy routes list` - 列出所有路由
- `aipass proxy routes create` - 创建新路由
- `aipass proxy routes update` - 更新路由配置
- `aipass proxy routes delete` - 删除路由
- `aipass proxy routes enable` - 启用路由
- `aipass proxy routes disable` - 禁用路由
- `aipass proxy routes token-rotate` - 轮换路由的认证令牌

##### 分组操作
- `aipass proxy groups list` - 列出路由中的所有分组
- `aipass proxy groups enable` - 启用分组中的所有目标
- `aipass proxy groups disable` - 禁用分组中的所有目标

##### 目标（Target）管理
- `aipass proxy targets list` - 列出路由中的所有目标
- `aipass proxy targets enable` - 启用特定目标
- `aipass proxy targets disable` - 禁用特定目标
- `aipass proxy targets priority` - 设置目标优先级
- `aipass proxy targets weight` - 设置目标权重（用于负载均衡）
- `aipass proxy targets reorder` - 重新排序目标

##### 应用配置
- `aipass proxy apply-to-tool` - 将代理配置应用到指定的 agent 应用

##### 使用统计
- `aipass proxy usage` - 查看代理使用统计
- `aipass proxy logs` - 查看代理日志

#### 2. 供应商管理 (`aipass provider`)

- `aipass provider list` - 列出所有供应商
- `aipass provider show <id>` - 显示供应商详情
- `aipass provider create` - 创建新供应商
  - 支持 `--supports-websockets` 标志
- `aipass provider update <id>` - 更新供应商配置
  - 支持 `--supports-websockets <true|false>` - 编辑供应商是否支持 WebSocket
  - 支持编辑所有供应商字段（标题、端点、认证等）
- `aipass provider delete <id>` - 删除供应商

#### 3. Vault 管理更新

- ✅ `aipass unlock` - 解锁 vault（原 `login` 命令已更名）
- `aipass vault status` - 查看 vault 状态
- `aipass vault lock` - 锁定 vault

#### 4. MCP 服务器 (`aipass-mcp-proxy`)

完整的 MCP 工具集，包含 23 个工具：

**服务器管理**
- `proxy_status` - 获取代理服务器状态
- `proxy_start` - 启动代理服务器
- `proxy_stop` - 停止代理服务器
- `proxy_restart` - 重启代理服务器

**路由管理**
- `proxy_list_routes` - 列出所有路由
- `proxy_create_route` - 创建新路由
- `proxy_update_route` - 更新路由
- `proxy_delete_route` - 删除路由
- `proxy_enable_route` - 启用路由
- `proxy_disable_route` - 禁用路由
- `proxy_token_rotate` - 轮换令牌

**分组管理**
- `proxy_list_groups` - 列出分组
- `proxy_enable_group` - 启用分组
- `proxy_disable_group` - 禁用分组

**目标管理**
- `proxy_list_targets` - 列出目标
- `proxy_enable_target` - 启用目标
- `proxy_disable_target` - 禁用目标
- `proxy_set_target_priority` - 设置目标优先级
- `proxy_set_target_weight` - 设置目标权重
- `proxy_reorder_targets` - 重新排序目标

**供应商管理**
- `proxy_list_providers` - 列出供应商
- `proxy_get_provider` - 获取供应商详情
- `proxy_create_provider` - 创建供应商
  - 支持 `supports_websockets` 字段
- `proxy_update_provider` - 更新供应商
  - 支持通过 `provider_data` 对象更新所有字段，包括 `supports_websockets`
- `proxy_delete_provider` - 删除供应商

**配置应用**
- `proxy_apply_to_tool` - 应用配置到工具

**统计和日志**
- `proxy_usage` - 获取使用统计
- `proxy_logs` - 获取日志

#### 5. Desktop 应用自动安装 CLI

- ✅ Desktop 启动时自动释放 CLI 二进制文件
- ✅ 自动安装到系统 PATH
  - macOS/Linux: `~/.local/bin/aipass` 或 `/usr/local/bin/aipass`
  - Windows: `%LOCALAPPDATA%\Programs\AIPass\aipass.exe`
- ✅ 自动设置可执行权限（Unix 系统）
- ✅ PATH 检测和警告

## 架构说明

### CLI 模块结构

```
crates/aipass-cli/src/commands/
├── proxy/               # 代理相关命令（已拆分）
│   ├── mod.rs          # 主入口和命令定义 (110 行)
│   ├── server.rs       # 服务器管理 (83 行)
│   ├── routes.rs       # 路由管理 (89 行)
│   ├── groups.rs       # 分组管理 (76 行)
│   ├── targets.rs      # 目标管理 (207 行)
│   └── usage.rs        # 统计和日志 (45 行)
├── provider.rs         # 供应商管理 (123 行)
└── vault.rs            # Vault 管理 (118 行)
```

所有模块都保持在合理大小（< 250 行），无需进一步拆分。

### MCP 服务器结构

```
crates/aipass-mcp-proxy/
└── src/
    └── lib.rs          # MCP 服务器实现和工具定义
```

### Desktop CLI 安装

```
apps/desktop/src-tauri/src/
└── cli_install.rs      # CLI 自动安装逻辑
    ├── ensure_cli_installed()  # 首次运行时安装
    ├── install_cli()           # 复制和配置 CLI
    └── is_cli_installed()      # 检查安装状态
```

在 `apps/desktop/src-tauri/src/lib.rs` 的启动流程中调用：
```rust
cli_install::ensure_cli_installed();
```

## 使用示例

### CLI 示例

```bash
# 查看代理状态
aipass proxy status

# 启动代理服务
aipass proxy start

# 列出所有路由
aipass proxy routes list

# 创建新路由
aipass proxy routes create "My Route" \
  --inbound openai \
  --upstream anthropic \
  --strategy fallback

# 启用某个分组
aipass proxy groups enable <route-id> <group-name>

# 设置目标优先级
aipass proxy targets priority <route-id> <target-id> 10

# 应用配置到工具
aipass proxy apply-to-tool <route-id> claude-desktop

# 更新供应商（启用 WebSocket）
aipass provider update <provider-id> --prefer-websocket true

# 解锁 vault
aipass unlock
```

### MCP 工具调用示例

```json
{
  "name": "proxy_status",
  "arguments": {}
}

{
  "name": "proxy_enable_group",
  "arguments": {
    "route_id": "uuid-here",
    "group": "production"
  }
}

{
  "name": "proxy_update_provider",
  "arguments": {
    "entry_id": "uuid-here",
    "provider_data": {
      "prefer_websocket": true
    }
  }
}
```

## 技术细节

### CLI 实现

- 使用 `clap` 进行命令行解析
- 通过 `AgentClient` 与本地 agent 通信
- 支持 JSON 和表格输出格式
- 完整的错误处理和用户友好的错误消息

### MCP 实现

- 异步 API 设计
- 标准 MCP 工具定义格式
- 通过 `AgentClient` 与本地 agent 通信
- 完整的参数验证和错误处理
- 支持所有代理操作的 CRUD 操作

### Desktop 集成

- 跨平台支持（macOS、Windows、Linux）
- 自动检测和创建安装目录
- 智能 PATH 检测
- 首次运行时自动安装
- 无需用户手动操作

## 编译状态

✅ 所有模块编译通过：
- `aipass-cli` - 编译成功
- `aipass-mcp-proxy` - 编译成功
- `aipass-desktop` - CLI 安装逻辑已集成

## 后续优化建议

1. **CLI 增强**
   - 添加交互式配置向导
   - 支持批量操作
   - 添加配置文件导入/导出

2. **MCP 增强**
   - 添加批量操作工具
   - 支持配置模板
   - 添加配置验证工具

3. **文档**
   - 添加详细的命令参考文档
   - 创建使用教程和最佳实践
   - 添加常见问题解答

4. **测试**
   - 添加单元测试
   - 添加集成测试
   - 添加 CLI 端到端测试
