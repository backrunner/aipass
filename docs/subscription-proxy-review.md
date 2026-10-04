# Subscription and local proxy review / 订阅与本地代理审查

Reviewed on 2026-10-03. AIPass baseline: `7988262`.
References: [Magpie main](https://github.com/yetone/magpie/tree/0f42934c1f8ec109921d522c76956bc4303ed961),
[Copilot CLI follow-up](https://github.com/yetone/magpie/tree/c9b7f95f904bc972bef4d7bdb382633b649bc695)
and [community adapters](https://github.com/magpie-community/plugins/tree/6fd444cf5b0a30c4046682b80d5ddc3a183bfac9).

## 本次落地的修复

| 范围 | 原问题与当前行为 |
| --- | --- |
| OpenAI 协议互转 | 补齐 Chat Completions ↔ Responses 的请求、响应和 SSE；处理工具 ID、并行参数增量、图片、schema、推理摘要、缓存和末尾 usage。中断或不完整 EOF 不伪造成功。 |
| 原生协议保真 | 社区上游与入口同协议时保留原生 Messages / Responses，包含 thinking 签名、加密推理和 custom tools；跨协议遇到不可表达语义明确报错。非流式聚合保留原生输出块。 |
| Codex 订阅 | 固定订阅 Responses 后端，`store:false`、强制流式，恢复非流式 JSON，合并 `output_item.done` 与 terminal output/usage，规范模型目录与客户端版本。额度用对应账号 token 和 workspace 请求，避免读到当前 CLI 的另一个账号。 |
| Claude 订阅 | 使用真正 Claude Code 子进程，保留调用方提示词，通过受认证的 MCP 桥交给调用方执行工具。保持多轮进程和等待中的工具结果，校验工具来源、完整结果集合、会话历史、过期与取消；独立凭据目录/Keychain，刷新归属和 CAS 写回。 |
| Copilot | 导入 editor OAuth grant 后交换短期 session token，使用可信动态 API 地址；读取模型策略与 supported endpoints，Auto 先取得 model/session token。另支持 Copilot CLI 配置/Keychain 导入，CLI token 使用独立客户端 headers 与账户 endpoint 查询，不误走编辑器交换；展示独立用量窗口。 |
| Gemini | 原生 generateContent/streamGenerateContent、API key header、模型目录、工具/图片/schema/thinking 转换。保存完整原生带签名工具回合并绑定目标、参数、顺序、文本和凭据代次，防止跨账号回放。 |
| API provider 差异 | 显式可信 provider profile 处理 OpenAI、DeepSeek、Kimi、Mistral、Gemini-compatible 的采样、token、thinking 和推理回放契约。Kimi K3 effort、K2.7-code 常开推理和 K2.6 开关分别处理。 |
| 额度路由 | 新增 quota-aware 策略；仅使用 300 秒内可信窗口。未知/过期值保持中性，按实际模型匹配 models/notModels，aside 展示窗口不参加路由。429 Retry-After 与额度耗尽分开；粘性只选择仍然可用的目标。 |
| 运行时管理 | 详情页实际接入 typed IPC → Agent → 加密 vault → 代理配置刷新。支持额度开关/间隔、单 provider 出站代理、余额 HTTP 请求/JSON 路径、被动健康和显式 webhook 测试；读取不回传已保存的秘密。 |
| Rust 订阅适配 | 参考下表服务的协议，在 Rust 实现登录/取消/验证码、模型发现、转换、刷新和用量链路。轮换凭据经 Agent 的账号代次、所有权及 revision CAS 确认后才启用。 |

## 社区 provider 覆盖

参考注册表共 11 个包、13 个 provider ID。它们仅作为协议参考，生产代码不接入这些包。

- `aipass-proxy-conversion/src/providers`：纯 Rust 请求、SSE、Connect/protobuf、AWS eventstream 转换。
- `aipass-agent/src/subscriptions`：原生异步 HTTP、登录、模型与用量发现、凭据轮换及取消。
- `aipass-agent/src/community.rs`：沿用已有 typed IPC 和加密账号记录，负责 vault CAS、会话归属与代理接入；`community` 名称保留用于已有记录兼容。
- Tauri/Svelte 仍只负责 UI；无 Node worker、AI SDK 包、动态 JavaScript 加载或运行时包下载。

| Provider ID | 特殊请求/响应契约 |
| --- | --- |
| `zcode` | Z.ai/BigModel Coding Plan、团队席位与 Start Plan；按账户模型目录和原生套餐路径请求。 |
| `qoder`, `qoder-cn` | 区域化设备登录与 job token；COSY RSA/AES 编解码、签名和 Chat Completions 归一化。 |
| `devin` | 浏览器或原生 CLI 登录；Connect/protobuf 请求与流转为 Chat Completions。 |
| `zed` | Zed 托管的 Anthropic/OpenAI/Gemini 模型；NDJSON/stream_ended 按各模型原生协议还原。 |
| `factory` | WorkOS 轮换、组织/区域信息及模型独立 SDK/API；保留 Droid 请求约束和多协议模型目录。 |
| `grok` | 真正 Grok Build CLI 登录/刷新；完整原生 auth bundle 由 Agent 加密保存，CLI 私有目录按操作恢复。 |
| `commandcode-plan` | 按计划区分 Provider API 与 Go alpha/generate 路径，沿用套餐的认证和转换。 |
| `mimo-app` | 小米账号 Cookie/passToken 认证，避免当成普通 bearer API key。 |
| `cursor` | HTTP/2 Connect、protobuf/blob 编解码和 MCP 工具调用；HTTP/2 同样经过所选出站代理。 |
| `kiro` | 社交/OIDC、原生 CLI 或 API key；AWS eventstream 解码为 Messages，保留 profile 和轮换归属。 |
| `workbuddy`, `workbuddy-ai` | 国内/国际独立地址和套餐；流式后端适配，原生桌面账号替换时拒绝静默换号。 |

入口：**连接订阅 → 更多服务商**。选择 provider 与其支持的登录方法；完成后条目出现在
供应商列表，可加入本地代理路由。详情页可刷新模型与额度；路由快速接入写入本地 token，
不会将内部账号标记当成上游 API key 导出。密钥、完整 OAuth/native bundle 和单 provider
代理密码均保存在加密 vault，模型及额度刷新复用同一账号所有权检查。

## 使用条件与明确边界

- 转换器运行在 **Rust Agent** 内，不依赖 Node/Bun 或社区 npm 包。
  Claude Code / Grok Build 使用对应官方 CLI；Cursor 的 CLI 导入刷新和 Devin 的可选 CLI 模型发现也仅调用真实厂商工具。
- 社区请求和刷新需要 **vault 已解锁**，确保轮换的新 refresh token 可以持久化后才继续使用。
  同一社区账号一次执行一个请求或刷新；忙时保留上一次有效额度，不把“忙”写成额度失败。
- 带签名或加密推理的原生历史不等于可移植历史。无法表达的跨协议 hosted tools、
  server-owned conversation、部分多模态工具输出等明确拒绝；原生同协议仍保留。
- Responses 下游 WebSocket 对订阅和转换目标使用现有 HTTP/SSE 桥，不能据此声称该上游
  本身支持原生 WS。生成已提交后的未知结果、部分写入或中断不能自动重放。
- Copilot 支持编辑器与 CLI 两种原生凭据导入；CLI 的 Keychain 读取当前在 macOS 验证，
  企业 GitHub 主机不按 github.com 凭据导入。补全和 premium 窗口不会误停整个聊天账户。
- Cursor 在 Rust reqwest HTTP/2 双向流中回复 blob/MCP 协议消息，只把调用方工具交给调用方执行。
  所有 HTTP 使用同一出站代理配置。服务端提出的 shell、文件操作等不会在本机执行。
- AWS eventstream 以完整帧和 HTTP 正常结束为结束依据，用量 metadata 可选；SSE、Connect 和 Zed
  则校验各自的终止标识。缺失帧、破损校验和和不完整工具参数不能伪造成功。
- 确定的认证拒绝可以刷新后重试一次；区域/组织修复仅在响应拒绝且尚未输出时进行，不重放模糊的已提交生成。
- 本地 fixture 不能证明真实账号订阅资格、扣费、实时模型可用性、远端一次性 token 轮换
  或所有平台兼容性。未用真实用户凭据进行付费生成；未发布任何产物。

## 2026-10-03 总体复查

本轮发现并修复以下问题，不能将上一轮测试通过理解为没有这些问题。

| 问题 | 修复与回归证据 |
| --- | --- |
| 已提交生成可能被重试 | 同一网络分片中的内容和错误会丢弃当前输出，使传输层误判为生成前拒绝；空或损坏的 HTTP 200 内容也被映射成可重试 502。现由转换器记录已解析的生成进度，仅显式拒绝且未产生内容时返回拒绝状态。公共代理的静默重试同样检查正文、推理、工具声明和完成事件；真实本地 HTTP 回归确认有输出后的错误不会调用备用账号。 |
| 工具调用损坏与数值溢出 | Devin/Qoder 原生工具参数在完成前校验完整 JSON 对象；Qoder 接受分开到达的 ID、名称和参数并拒绝畸形块；Command Code 补足缺失调用 ID、拒绝完成后的输出。异常 usage 使用检查加法返回错误。 |
| IPC 标签错位 | 恢复 `official_accounts.refresh` 的原有分发，纠正 Claude 内部消息标签，为新增社区与运行时消息补齐明确 wire tags。测试每个标签与操作名、往返类型以及旧 JSON 请求。 |
| 凭据归属检查失真 | 分别比较已知账户字段与 JWT claims；允许同一账号增加 profile/email，拒绝在保留旧 accountId 的同时切换 token subject。 |
| 登录保存中断留下不完整条目 | 条目与原生认证 bundle 改为同一加密记录首次写入。回归验证首次记录已含凭据、摘要与磁盘不暴露认证明文。 |

## 验证记录

当前 Rust 移植的 macOS 验证结果在本节单独记录。此前 Node worker 和上游 JavaScript 测试的结果不作为 Rust 实现通过的依据。

- Rust 转换单元测试：工具与多模态历史、推理、原生签名、late usage、AWS CRC、Connect protobuf、Qoder XML、异常 EOF。
- Rust 集成测试：Cursor HTTP/2 同时上传/下载与 blob 回复、明确出站代理、凭据 ACK 拒绝、数据背压、取消、账号 UID/profile 不变性、HTTP 200 内额度错误映射为 429。
- 代理回归：原有 Chat/Responses/Messages 互转、订阅桥接、额度按模型路由、凭据代次失效。
- 本地 macOS / Homebrew Rust：Agent、proxy、conversion、protocol、registry、vault、桌面 Rust 共 **685 passed / 1 ignored**；其中转换层 91 项，Agent 245 项，proxy 208 项。唯一 ignored 为原有交互浏览器 fixture。公共代理最终改动后重跑了其全部 208 项测试，其余结果来自本轮完整七 crate 测试。
- 桌面前端 **241 passed / 34 files**；Svelte 检查 0 errors / 0 warnings；生产构建通过（现有 chunk size 提示保留）。
- 定向 Rust 格式检查通过；`PATH` 不含任何外部可执行文件的子进程测试仍能加载完整 Rust 订阅目录，确认无需 Node。
- Agent、proxy、conversion、桌面 Rust 的 all-targets Clippy（`--no-deps -D warnings`）通过。Agent、桌面端、CLI 的 macOS 构建通过；桌面使用 `custom-protocol` 嵌入生产前端资源。
- 原始记录：`/tmp/aipass-native-subscription-final-tests.log`、`/tmp/aipass-native-desktop-tests.log`、`/tmp/aipass-native-desktop-typecheck.log`、`/tmp/aipass-native-desktop-build.log`、`/tmp/aipass-native-subscription-clippy.log`、`/tmp/aipass-native-subscription-build.log`。
- 本轮复查记录：`/tmp/aipass-subscription-rereview-final-tests.log`、`/tmp/aipass-subscription-rereview-proxy-tests.log`、`/tmp/aipass-subscription-rereview-ui-tests.log`、`/tmp/aipass-subscription-rereview-ui-check.log`、`/tmp/aipass-subscription-rereview-clippy.log`、`/tmp/aipass-subscription-rereview-build.log`。七个相关 crate 的最终 Clippy 与 Agent/桌面/CLI 构建通过。
- 修复后的 Rust 构建在隔离 Tauri 副本中通过启动、渲染 DOM、前端与原生事件循环响应、20 轮 Agent IPC 和 960×640 窗口检查；记录位于 `/tmp/aipass-runtime-recheck-20261003/result.json`（`ok: true`），同目录保留二进制 SHA-256。首次复查目录过长，106 字节 Unix socket 路径触发 `AF_UNIX path too long`；失败诊断保留在 `/tmp/aipass-runtime-subscription-rereview-20261003`，使用相同二进制缩短 fixture 路径后通过。此前电脑控制工具返回 `cgWindowNotFound`，视觉与点击验收仍未完成。
- 未用真实账号完成所有 provider 的付费生成、实际扣费或远端一次性 token 轮换验证；需要有对应订阅资格的账号执行这些验收。
- 未运行 DMG/updater 发布门禁，也未提交、推送或发布。

## English summary

The 13 referenced community provider IDs are implemented as native Rust adapters.
Magpie community implementations supply protocol references and static metadata,
not runtime packages. The Rust Agent owns authentication, HTTP, model discovery,
usage, cancellation and encrypted credential rotation. Pure protocol conversion
lives in `aipass-proxy-conversion`. Tauri remains the UI/IPC layer.

There is no Node converter process or dynamic plugin loading. Genuine vendor CLIs
remain where their own authentication or model discovery requires them. All native
HTTP requests share the configured outbound proxy. Unsupported cross-protocol
semantics fail explicitly; ambiguous submitted generations are never replayed.
The original MIT attributions are preserved in `NOTICE` and the conversion crate's
`third-party` directory. Tests use synthetic accounts and local fixtures; live
subscription eligibility, billing and all remote login flows remain unverified.
