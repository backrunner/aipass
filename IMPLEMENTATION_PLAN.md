# AIPass CLI 和 MCP 本地代理管理实现计划

## 概述
扩展 AIPass CLI 和 MCP 服务器以支持本地代理的完整管理功能，包括：
- 对分组的操作（切换、开关、编辑优先级等）
- 对供应商的操作（编辑配置、WS支持等）
- 将 `login` 命令改为 `unlock`
- Desktop 初安装时自动释放 CLI 并使其全局可访问

## 实现步骤

### 1. CLI 命令扩展

#### 1.1 重命名 login 为 unlock
- [x] 将 `Command::Login` 改为 `Command::Unlock`
- [x] 更新文档和帮助信息

#### 1.2 添加 Proxy 子命令组
```rust
Proxy {
    #[command(subcommand)]
    command: ProxyCommand,
}
```

子命令包括：
- `status` - 查看代理服务器状态
- `start` - 启动代理服务器
- `stop` - 停止代理服务器
- `config` - 配置管理
- `route` - 路由管理
- `group` - 分组管理
- `logs` - 日志查看
- `usage` - 使用统计

#### 1.3 添加 Provider 管理扩展
扩展现有的 provider 命令，添加：
- `--max-concurrent` - 设置最大并发数
- `--supports-websockets` - 设置是否支持 WebSocket
- `--prefer-websockets` - 设置是否优先使用 WebSocket

### 2. MCP 服务器实现

创建新的 MCP 服务器 crate: `aipass-mcp-server`

#### 2.1 核心功能
- 代理状态查询
- 路由管理（CRUD）
- 分组管理
- 供应商配置管理
- 使用统计查询

#### 2.2 MCP 工具定义
- `aipass_proxy_status` - 获取代理状态
- `aipass_proxy_start` - 启动代理
- `aipass_proxy_stop` - 停止代理
- `aipass_route_list` - 列出路由
- `aipass_route_create` - 创建路由
- `aipass_route_update` - 更新路由
- `aipass_route_delete` - 删除路由
- `aipass_route_enable` - 启用/禁用路由
- `aipass_group_list` - 列出分组
- `aipass_group_switch` - 切换分组
- `aipass_group_priority` - 设置分组优先级
- `aipass_provider_update_ws` - 更新供应商 WS 配置
- `aipass_usage_summary` - 使用统计

### 3. Desktop 自动安装 CLI

#### 3.1 在 Desktop 应用中添加 CLI 安装逻辑
- 检测 CLI 是否已安装
- 如果未安装，从内置资源释放 CLI
- 设置正确的权限（Unix: +x）
- 添加到 PATH（可选，提示用户）

#### 3.2 实现位置
- `apps/desktop/src-tauri/src/self_install.rs` - 扩展现有安装逻辑
- 在首次启动时检查并安装 CLI

### 4. Agent Protocol 扩展

添加新的请求类型（如果需要）：
- `ServerGroupList` - 列出分组
- `ServerGroupSwitch` - 切换分组
- `ProviderUpdateConcurrency` - 更新供应商并发配置

## 技术细节

### CLI 架构
```
aipass-cli/
  src/
    main.rs          - 主入口，命令定义
    dispatch.rs      - 命令分发逻辑
    proxy.rs         - 新增：代理管理命令实现
```

### MCP 服务器架构
```
crates/aipass-mcp-server/
  src/
    lib.rs           - MCP 服务器主逻辑
    tools.rs         - MCP 工具定义
    agent_client.rs  - Agent 客户端封装
```

### Desktop 集成
```
apps/desktop/src-tauri/src/
  cli_installer.rs   - 新增：CLI 安装逻辑
  self_install.rs    - 扩展：集成 CLI 安装
```

## 测试计划

1. CLI 命令测试
   - 所有新命令的单元测试
   - 集成测试验证与 Agent 的交互

2. MCP 服务器测试
   - 每个工具的功能测试
   - 权限和认证测试

3. Desktop 安装测试
   - 首次安装流程测试
   - 跨平台测试（macOS, Windows, Linux）

## 实现优先级

1. **高优先级**
   - Login -> Unlock 重命名
   - CLI Proxy 基础命令（status, start, stop, config）
   - Desktop CLI 自动安装

2. **中优先级**
   - CLI 路由管理命令
   - CLI 供应商配置扩展
   - MCP 服务器核心功能

3. **低优先级**
   - MCP 服务器高级功能
   - 使用统计和日志查看
   - 性能优化

## 兼容性考虑

- 保持与现有 API 的向后兼容
- CLI 命令应支持 JSON 输出格式
- MCP 工具需要明确的错误处理
