# OAuth 刷新复核（2026-10-03）

本次核对当前支持的官方账号和 13 个订阅 provider，修复实际发现的刷新缺口。认证、持久化和转换仍由 Rust 实现，没有接入 community JavaScript 包。

## 对照版本

- Magpie：`2d5f9c748ed83232c10bfa7f6204d6abfd1194a3`，通过远程 HEAD 和源码核对。
- Community：`873449a2f40379014a9bed31b4abd556d70010d0`，通过远程 HEAD 和源码核对。
- 主要参考：[官方账号与 Copilot 会话](https://github.com/yetone/magpie/blob/2d5f9c748ed83232c10bfa7f6204d6abfd1194a3/internal/provider/account.go)、[Factory 刷新](https://github.com/yetone/magpie/blob/2d5f9c748ed83232c10bfa7f6204d6abfd1194a3/internal/provider/factory.go)、[Kiro 原生凭据](https://github.com/yetone/magpie/blob/2d5f9c748ed83232c10bfa7f6204d6abfd1194a3/internal/provider/kiro.go)。Community 逐包核对 `packages/*/index.mjs`，尤其是 [Qoder](https://github.com/magpie-community/plugins/blob/873449a2f40379014a9bed31b4abd556d70010d0/packages/qoder/index.mjs)、[WorkBuddy](https://github.com/magpie-community/plugins/blob/873449a2f40379014a9bed31b4abd556d70010d0/packages/workbuddy/index.mjs)、[Factory](https://github.com/magpie-community/plugins/blob/873449a2f40379014a9bed31b4abd556d70010d0/packages/factory/index.mjs) 和 [Cursor](https://github.com/magpie-community/plugins/blob/873449a2f40379014a9bed31b4abd556d70010d0/packages/cursor/index.mjs)。

## 已修复的缺口

1. **403 被误判为永久失效**：Codex/xAI 不再因裸 403、HTML 拦截页或网关故障清空凭据。401 或明确的 grant 失效码才触发重新登录；429/5xx 不触发凭据吊销。错误日志不包含上游自由文本或令牌。
2. **请求取消丢失已轮换令牌**：Factory、Qoder、WorkBuddy、Kiro、Grok Build 的刷新临界区在有界超时内继续读取响应并等待加密保存 ACK。取消后的 ACK 仍可交付，原生成请求继续保持取消；保存被拒绝时不能使用新令牌。
3. **持久化失败重复消耗旧 refresh token**：官方账号保留绑定账号 ID、旧令牌和 generation 的刷新结果；订阅账号保留绑定 generation/revision 的待保存更新。恢复后重试保存，不再请求一次已消耗的 grant。丢失 ACK 时读回确认精确版本；新登录/新编辑使旧恢复结果失效。锁库清除恢复缓存。
4. **CLI 镜像先于 vault**：官方刷新先保存完整托管凭据，再同步 CLI 文件和 secret 镜像。代理读取关联主凭据的托管 OAuth 记录，避免镜像失败后继续使用旧令牌；其他 API key 不会继承此令牌。后台幂等修复镜像，并移除已过期/需重新登录的运行时凭据。
5. **有效期和身份检查不足**：优先采用 access JWT 的更早过期时间；保存重试保留第一次收到响应时的绝对有效期。刷新不能改变已知 subject、email 或 ChatGPT workspace。省略 refresh token/id token 时保留旧值，空白 refresh token 按省略处理。
6. **Provider 特殊处理**：保留 Qoder job 毫秒与 device 秒的差异，限制生命周期溢出；WorkBuddy 检查 refresh grant 的有效期；Factory 临时刷新失败只允许继续使用尚未过期的 access token；Grok Build 拒绝 CLI 刷新后仍过期的结果。
7. **Kiro CLI 所有权与轮换**：同一原生 grant 优先由 `kiro-cli debug refresh-auth-token` 刷新。读取完整 token pair 后核对 DB key/profile；CLI 省略 profile 时，用新 token 查询并确认原 profile。识别原生已更新的 generation，避免随后再刷新一次。旧原生缓存的指纹包含 access 与 refresh 两者，写回仍使用 SQLite CAS/原子文件替换。
8. **Claude/Copilot**：Claude 原生完整 grant 保存失败会后台重试，未保存的轮换不会因普通会话 TTL 被丢弃；锁库、移除和身份冲突不恢复旧账号。Copilot editor session 提前两分钟重新交换，缓存按 GitHub grant 与 CLI/editor 类型隔离，并发共用一次交换，不缓存已过期的结果。

### 提交前二次复核（2026-10-04）

- Claude 现在比较完整 native grant，避免 access token 不变时漏存 refresh token 的轮换。保存核对完整旧 grant 的哈希；旧进程不能覆盖其他进程已提交的轮换，完全相同的更新可以幂等重放并补齐镜像。进程容量淘汰也保留未保存的 grant，macOS 同有效期时优先读取 CLI 的 Keychain 结果。
- Kiro 原生导入不再要求新 grant 的有效期严格大于旧值：账号一致、原生 grant 已变且仍有效时，采用完整 pair；仍拒绝已知旧原生版本和过期版本，避免相同有效期的 refresh-only 轮换被忽略。
- 回归覆盖完整加密 grant 保存、丢失 ACK 后重放、旧 writer 冲突、access 不变时的 pending 判断，以及 Kiro 相同/较短有效期的原生轮换。
- 官方 device 登录在交换成功时固定 access token 的绝对有效期，保存重试不会延长它。已交换的完整 grant 不再随 device code 到期被淘汰；只在保存成功、取消或锁库时清除。首次登录及重新登录先保存完整托管凭据，CLI 镜像和代理重载失败不会重放已经保存的旧登录 grant。回归同时验证真实 vault 的磁盘读回、重新登录去重和代理重载失败。

## Provider 刷新契约

| 账号/provider | 刷新方式与边界 |
| --- | --- |
| Codex/ChatGPT | OAuth refresh grant，提前五分钟；缺失新 refresh/id token 保留旧值；原生工作区与 subject 必须一致 |
| xAI 官方 OAuth | 校验 discovery 的 HTTPS/xAI endpoint；refresh grant 和原生身份绑定 |
| Claude Code | 由隔离的官方 CLI 刷新；Agent 保存完整原生凭据并更新镜像，失败可重试 |
| GitHub Copilot | Editor grant 交换短期 session；CLI grant 使用独立 integration，不混用 editor session |
| Factory | WorkOS refresh grant，提前两分钟；组织修复产生的轮换也必须先保存 |
| Qoder、Qoder CN | Job/device 两种 token pair；job `expires_in` 为毫秒，device 为秒；两种轮换都要求完整新 pair |
| WorkBuddy、WorkBuddy AI | `X-Refresh-Token` 刷新；保留未返回的字段，检查 access/refresh 两类有效期 |
| Kiro | Social、Identity Center、external IdP 各用对应 grant/endpoint；CLI 优先、完整读取、身份校验及 CAS 写回 |
| Grok Build | 官方 CLI 在账号独立的临时 home 中刷新完整 bundle；保存后才用于代理 |
| Cursor | CLI 用 `status` 刷新并重读；API key 交换 access；浏览器 session 到期要求重新登录，community 同样未定义其 refresh grant API |
| MiMo App | 用已保存的小米账号凭据重新建立 cookie session，并核对 userId |
| ZCode | 原生凭据重读并核对身份；组织 key 与 Start Plan JWT 分开，过期 JWT 提示重新登录 |
| Zed | 按请求从账号凭据取得短期 LLM token；不把其持久账号凭据当作标准 OAuth refresh token |
| Devin、Command Code Plan | 持久账号/API grant；不对这些 key 发起不存在的 OAuth refresh 请求 |

## 初次 OAuth 专项验证

- macOS 原生 Rust：Agent 263 项通过、1 项忽略；Proxy 210 项通过；Vault 51 项通过，共 524 项通过。
- 最后补充的 OAuth/订阅测试再次运行；OAuth 专项覆盖过期判断、省略令牌、stale refresh、新登录、保存顺序、原始有效期和镜像幂等修复。
- 本地 HTTP fixture 验证取消期间返回轮换凭据后仍等待 ACK；真实临时 vault 的只读目录故障验证订阅 auth 写失败后的恢复。
- Copilot 并发交换、账号隔离、提前刷新和过期结果测试通过。
- Agent/Proxy `clippy --all-targets --no-deps -- -D warnings` 通过；修改的 Rust 文件格式检查及 `git diff --check` 通过。
- 按 build.rs 的源码哈希检查重建内嵌 control panel，避免上一轮共享 UI 文案变化使 Cargo 构建嵌入过期资源。
- 日志：`/tmp/aipass-oauth-magpie-final-tests.log`、`/tmp/aipass-oauth-focused-final.log`、`/tmp/aipass-oauth-copilot-final.log`、`/tmp/aipass-oauth-magpie-clippy.log`。

## 验证边界

未使用真实账号完成线上登录、refresh token 轮换或付费生成；未发布生产桌面 artifact。以上是源码对照、模拟上游、加密持久化故障和 macOS 自动化测试证据。提交前另行执行完整 workspace、Node 与 macOS bundle/runtime gate，远程 CI 结果以对应提交的 GitHub Actions 为准。

服务商撤销授权、刷新响应在网络中丢失，以及锁库/退出跨越上游轮换时，可能需要重新登录。恢复缓存只在授权的解锁会话内有效，不绕过锁库保存秘密。独立运行的官方 CLI 没有与 AIPass 共享全程跨进程 OAuth 锁；原生重新读取、CLI 优先刷新和 CAS 写回缩小竞态并拒绝错误身份，不能据此承诺服务商或外部进程永远无故障。
