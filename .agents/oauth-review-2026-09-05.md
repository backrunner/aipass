# OAuth 复核（2026-09-05）

对照源码：[CC Switch `db41d701879592b8eca938cbe5c5ac28dd732b9f`](https://github.com/farion1231/cc-switch/tree/db41d701879592b8eca938cbe5c5ac28dd732b9f)。重点文件是 `src-tauri/src/proxy/providers/codex_oauth_auth.rs` 和 `xai_oauth_auth.rs`。

## 对照结论

- Codex 的客户端 ID、设备码请求/轮询端点、验证页、服务端 PKCE verifier 和 token exchange redirect URI 一致。403/404 表示尚未授权，410 表示设备码过期。
- Grok 的 OIDC issuer、客户端 ID、scope、设备授权及 refresh grant 一致；AIPass 校验 discovery 和验证页的 HTTPS/xAI 域名，并拒绝自动 HTTP 重定向。
- CC Switch 对同一账户刷新加锁、刷新前检查 CLI token generation，并在失效时重新读取 CLI。AIPass 的后台刷新单线程串行，但原来的失败结果和本地账户匹配缺少保护，本次已补齐。
- AIPass 继续由 agent 持有令牌、vault 加密保存完整令牌、IPC 只返回不含令牌的账户摘要。官方 OAuth 代理端点固定到对应提供商后端。

## 已修复

1. **设备码取消和重试**：缓存成功交换的令牌直到保存完成，避免重复消费一次性授权码；并发/过早轮询不再重复请求上游；取消与提交串行，锁库清除待处理登录。UI 立即使旧请求失效，关闭期间迟到的 start、poll 及倒计时过期结果均不再恢复登录。
2. **刷新竞态**：成功与失败都同时核对旧 refresh token 和时间戳，防止旧失败清空刚重新登录的账户。Codex 同时识别嵌套 `error.code`、字符串 `error` 和顶层 `code`，包含 `invalid_grant`。
3. **CLI 轮换及账户隔离**：刷新前读取同账户的更新 CLI 凭据，遇到失效再次读取；Codex 核对工作区和 OIDC subject，Grok 核对身份，拒绝不完整令牌。避免覆盖或采纳另一账户的凭据。
4. **ChatGPT 工作区去重**：同一邮箱不同工作区保留独立 provider entry、token 和 `chatgpt-account-id` 请求头。
5. **持久化保护**：新建 CLI 备份使用 vault backup key 加密；原生文件损坏/不可读时保留原文件；原生文件写入失败不再阻止保存托管令牌；刷新先保存托管账户，再更新 provider secret。
6. **响应和日志**：成功响应也限制为 64 KiB，解析错误不回显令牌，令牌 bundle 的 Debug 输出脱敏并在释放时清零；429 按可重试/放慢轮询处理。

## 验证

- `cargo test -p aipass-agent -p aipass-vault -p aipass-crypto -p aipass-sync -p aipass-config-writers` 通过。
- 最终补充测试后，`cargo test -p aipass-agent oauth --lib`：27 项通过。
- `cargo clippy -p aipass-agent --all-targets -- -D warnings` 通过。
- `pnpm --dir apps/desktop test src/lib/components/providers/OAuthConnectDialog.test.ts`：6 项通过。
- `pnpm --dir apps/desktop typecheck`：0 errors、0 warnings。
- 使用真实组件及模拟 IPC，在浏览器 `960×640` 检查选择页、设备码页和 12 账户列表：无横向溢出，列表可滚动，底部操作可见。这不等同于打包 Tauri 应用的端到端验证。

## 验证边界

未使用真实 ChatGPT/Grok 账号执行浏览器授权、线上刷新或订阅请求，未执行发布构建或完整跨平台 CI。服务商的未公开设备端点仍可能变化，不能仅凭本地测试承诺线上绝对无误。

本次未读取或修改用户真实的 CLI 凭据。历史明文 `.aipass-backups` 未被迁移；新建备份加密。官方 CLI 正在独立运行时，跨进程读取与原子写入之间仍存在竞态窗口，本次按账户和 generation 检查缩小风险，没有与官方 CLI 建立跨进程锁协议。


## UI 与交互重做

后续按 UI 复核要求重做了连接页、两步授权页和账号管理页，复用仓库已有的 Codex/Grok 图标、颜色与按钮。提供商选择后先呈现验证码，用户主动点击授权按钮才打开系统浏览器。

补齐准备登录、请求失败、验证码过期、重试、账号列表加载/加载失败/空列表、复制与打开浏览器失败的反馈。重新登录保留账号提示，取消返回原列表；列表显示工作区，移除前解释关联服务及代理路由的影响，并进行就地确认。移除成功后的列表刷新失败不会阻止 host 更新或让已移除项继续显示。

新增 Tauri 授权页打开命令，仅接受提供商 HTTPS 地址，不使用 shell 拼接 URL。验证包括 11 项组件交互测试、桌面类型检查、浏览器 URL 白名单 Rust 测试及 desktop Clippy。在 960×640 的真实组件预览中检查了中英文、深浅主题、授权/失败/空列表/长列表与移除确认。浏览器预览使用模拟 IPC，不表示真实账号已完成授权。
