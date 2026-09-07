# 本地日志与排障

AIPass 的核心操作统一经过 Agent，桌面、CLI 和浏览器扩展共用操作日志入口。日志自动保存在本机，无需开启调试模式。

## 日志位置

| 内容 | 文件 / 入口 |
| --- | --- |
| 提供商、密钥、保险库、同步、OAuth、代理配置、工具配置等 Agent 请求 | `agent.log` |
| 客户端发送、响应、IPC 超时或连接失败 | `client.log` |
| 桌面启动、窗口、托盘和更新检查/下载/安装事件 | `desktop.log` |
| 浏览器 Native Messaging 请求和响应 | `native-host.log` |
| 代理接入、请求、上游尝试、启动、停止、配置重载 | 保险库目录下 `proxy-usage.sqlite` 的 `proxy_diagnostics` 表；桌面本地代理的日志窗口读取最近 1,000 条 |
| 提供商及凭据变更的加密审计 | 保险库目录下 `audit/*.aipaudit` |

文本日志默认目录：macOS 为 `~/Library/Logs/AIPass`，Linux 为 `${XDG_DATA_HOME:-~/.local/share}/desktop/logs`，Windows 为 `%LOCALAPPDATA%\aipass\desktop\data\logs`。所有组件均支持环境变量 `AIPASS_LOG_DIR` 覆盖目录。

升级前的 `agent-YYYY-MM-DD.log`、`native-host-YYYY-MM-DD.log` 仍留在原平台数据目录中；新日志写到上述统一位置。旧文件不自动合并或删除。

## 如何追踪一次操作

`agent.log` 记录事件名、开始/完成/失败、UTC 时间、PID、`request_id`、对象 UUID、耗时和错误码。例如 `provider.add` 完成时记录新提供商的 UUID；之后可以按该 `resource_id` 查找编辑、归档、恢复和删除。

同一个 `request_id` 贯穿客户端发送、Agent 执行以及原生扩展桥接；工具配置日志还包含 `operation_id`。`transport_failed` 表示客户端没有收到响应，`delivery_failed` 表示 Agent 已执行但响应发送失败，因此不能直接把传输失败视为“未修改数据”。

同步和探测还记录经过白名单筛选的结果状态、HTTP 状态码和计数；业务失败不会仅因 IPC 成功而显示为操作成功。后台文件夹/WebDAV 同步也记录开始和结束。成功的状态、心跳、代理日志和用量轮询不逐条写入操作日志，失败仍有记录。

代理日志用 `request_id` 关联各次上游尝试，并记录 route/target/provider UUID、HTTP 状态、结果、耗时。`proxy.http.response_headers` 只表示响应头已返回；流式请求的最终结果查看 `proxy.request.completed` 和 `proxy.attempt.completed`。WebSocket 每个生成请求使用独立 UUID。模型查询及鉴权拒绝也保留接入结果。

`proxy.upstream.rejected` 记录上游 HTTP 状态与错误说明，包括 HTTP/model discovery 拒绝、WS 握手和 WS/SSE 错误事件；即使后续服务商回退成功，原始失败原因仍保留。错误响应最多读取 16 KiB、等待 1 秒，日志说明最多 2,048 个字符；JSON 仅提取错误代码、类型和说明，非 JSON 响应保留脱敏后的有限片段。超长或无法解析的 JSON 只记录固定提示，避免把其他正文误作错误。桌面日志窗口通过已解锁的服务商列表解析 UUID，显示服务商名称和渠道标签。

日志支持本地排障，不是不可篡改的审计证明。进程被强制终止时可能只有开始记录；正常异常展开会记录 `interrupted`，panic 只写代码位置。

## 排查 Codex 工具缺失与 WebSocket 异常

以下诊断随代理请求自动记录，需要运行包含这些改动的 Agent。桌面本地代理的日志窗口可直接查看；完整保留范围仍是 `proxy-usage.sqlite` 中最近 10,000 条诊断。

| 事件 | 排查用途 |
| --- | --- |
| `proxy.tools.summary` | 对照 `http_inbound`、`ws_bridge_prepared`、`converted_upstream`、`ws_upstream` 阶段的工具数量及类型。`ws_bridge_prepared` 位于桥接器恢复上下文之后，`ws_upstream` 是即将原样发送的 WS 请求。`tools_present=false` 表示未提供工具列表，`available=false` 表示无法解析摘要，不能当成工具数量为零。 |
| `proxy.request.forwarding` | 按请求 ID 查看目标、尝试次数、入站与上游协议，以及是否发生转换。WS 上游包含 `connection_id`，多个请求的值相同表示复用了同一连接。原生直通请求沿用入口工具摘要；转换后另记工具摘要。 |
| `proxy.target.cooldown` | 查看临时拉黑的目标、`cooldown_ms` 时长与 `reopen_count` 累计开启次数。默认连续失败 3 次拉黑 30 秒，恢复尝试再次失败立即加倍，最长 15 分钟；已在途的其他失败不延长同一次拉黑。 |
| `proxy.responses.summary` | 每个生成请求结束或中断时，汇总 Responses 事件、文本/工具增量、终止事件数量及顺序异常。`transport` 区分原生 WS 上游与 HTTP/SSE 上游；不对转换后的下游流作同样的顺序判断。 |
| `proxy.websocket.transport` | 区分连接失败、握手超时、HTTP 拒绝、无效 Upgrade、连接成功、`reused` 连接复用和 HTTP 桥接回退；能够提取时保留 `os_error` 数字，不保存原始错误。 |
| `proxy.websocket.closed` | 查看断开方向、固定原因类别、WS 关闭码，以及断开时尚未完成的请求数。 |

工具类型只统计 `function`、`custom`、`shell`/`local_shell`、`namespace` 和 `other`，命名空间包含其子工具计数。`exec` 仅统计名称匹配 `exec`、`exec_command`、`shell` 的定义数，不保存任何工具名称，也不保证这些工具实际可执行。

`delta_without_item>0` 表示某个带 `output_index` 的文本/工具增量出现时，没有对应的活动输出项；`sequence_regressions>0` 表示请求内序号重复或倒退。缺少索引的增量单独计入 `unindexed_deltas`；`tracking_limited=true` 表示达到观察上限，不能据此断言序列完整。`terminal=0` 需结合关闭原因判断是上游中断还是客户端取消。这些计数用于定位，不改变转发、重试或熔断行为。

复现后先记录本地时间，在日志中找到对应的 `proxy.websocket.request.started` 或 `proxy.request.started`，再按 `request_id` 对照工具摘要、响应摘要及上游尝试。握手阶段的 `request_id` 是代理生成的连接 ID；`proxy.websocket.request.started` 和 `proxy.websocket.bridge.request` 用 `connection_id` 将同一连接上的生成请求关联起来。混合传输路径的上游 WS 尝试与 HTTP 回退使用同一个生成请求 ID。

只写每请求摘要及连接状态，不逐 token 写日志。摘要解析跳过提示词、工具说明和参数 schema；不保存响应文本、工具参数、调用结果、外部响应 ID 或 WS 关闭原因原文。

会话在成功降级后优先保留原渠道，其他请求优先使用稳定目标，再考虑近期恢复和仍在降级的目标。冷却结束仅允许一个生成请求尝试恢复；连续两个新请求成功后解除降级，最近失败历史保留 10 分钟以避免反复切换。代理不会额外生成计费请求来探测恢复，等待重试也不会绕过拉黑。HTTP/SSE/WS 的响应 ID 与会话绑定仅在内存中限量保留，闲置 30 分钟后过期，不写入日志。

## 保留与隐私

- 文本日志每个组件单文件 10 MiB，达到上限后继续轮转，最多保留当前文件和 10 个历史文件。多进程通过文件锁协调写入；Unix 目录权限为 `0700`，文件为 `0600`。
- 代理诊断保留最近 10,000 条；停止、重启和清空用量不会清除诊断。`proxy_usage`、`proxy_attempts` 是既有用量历史，清空用量时删除；它们与诊断保留策略独立。
- 加密审计沿用保险库的加密、同步及备份规则。
- 操作/代理诊断不保存 API Key、密码、令牌、正常请求/响应正文、提供商标题、上游 URL、模型名称、Header 值或配置内容。上游错误说明须经过统一错误字段提取、凭据/Header 脱敏、URL 过滤及长度限制；其他网络错误仍记录固定类别和代码，不能直接格式化任意请求或响应到日志。
- 无法写入日志时会向 stderr 输出不含原始内容的失败提示；磁盘故障下不保证日志完整。

新增 Agent 操作必须补充 `AgentRequest::event_name()` 的穷尽匹配；能关联对象 UUID 的操作还应补充 `operation_log::resource_id()`。新增代理传输必须沿用请求 ID、上游尝试和诊断存储入口。日志回归覆盖提供商操作链、业务失败、异常中断、脱敏、轮转、并发写入、代理重启和成功回退。

## 更新后仍出现旧行为

后台 Agent 是常驻进程，替换 app 磁盘文件并不等于已重启 Agent。可对照日志 PID/启动时间与桌面构建时间；`agent.compatibility outcome=replacing` 表示新版客户端发现较旧协议并通过鉴权 IPC 关闭旧进程，随后启动匹配的 Agent。该过程会锁定旧会话，需要在新 Agent 上重新解锁。新客户端不接受旧协议成功响应，也不向旧协议重试配置写入。

Codex 预览只读取配置文件，不发现或读取 JSONL/SQLite 历史；实际应用配置时才规划和执行历史迁移。`delivery_failed` 紧跟大量预览文件通常表示仍在运行旧 Agent，不能仅凭 app 文件已替换判断修复是否生效。
