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

日志支持本地排障，不是不可篡改的审计证明。进程被强制终止时可能只有开始记录；正常异常展开会记录 `interrupted`，panic 只写代码位置。

## 保留与隐私

- 文本日志每个组件单文件 10 MiB，达到上限后继续轮转，最多保留当前文件和 10 个历史文件。多进程通过文件锁协调写入；Unix 目录权限为 `0700`，文件为 `0600`。
- 代理诊断保留最近 10,000 条；停止、重启和清空用量不会清除诊断。`proxy_usage`、`proxy_attempts` 是既有用量历史，清空用量时删除；它们与诊断保留策略独立。
- 加密审计沿用保险库的加密、同步及备份规则。
- 操作/代理诊断不保存 API Key、密码、令牌、请求/响应正文、提供商标题、上游 URL、模型名称、Header 值或配置内容。错误优先记录固定类别和代码，不能把任意网络错误、请求或响应直接格式化到日志。
- 无法写入日志时会向 stderr 输出不含原始内容的失败提示；磁盘故障下不保证日志完整。

新增 Agent 操作必须补充 `AgentRequest::event_name()` 的穷尽匹配；能关联对象 UUID 的操作还应补充 `operation_log::resource_id()`。新增代理传输必须沿用请求 ID、上游尝试和诊断存储入口。日志回归覆盖提供商操作链、业务失败、异常中断、脱敏、轮转、并发写入、代理重启和成功回退。
