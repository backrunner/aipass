# Codex、AIPass 与上游的 WS 会话

检查日期：2026-09-07。连接生命周期以 Codex 客户端为边界。

## 转发规则

| 路径 | 请求与状态 | 连接生命周期 |
| --- | --- | --- |
| Codex WS → 原生 Responses WS | 选中上游后固定连接，应用帧原样转发，不改写工具、输入、响应 ID 或 `stream_id`。路由包含其他协议或开启转换选项，本身不触发转换。 | 一对一；客户端关闭时关闭上游，新客户端连接不复用旧连接。 |
| Codex WS → fallback/协议桥接 | 按 lane 排队、维护本连接内的响应链，将 `previous_response_id` 与新输入恢复为完整输入，再选择 provider。 | 只在当前客户端连接内保留成功的 WS 上游，空闲 120 秒过期，最多 32 个；客户端关闭或凭据、接口等传输配置变化时全部清理；仅 WS 偏好变化不终止连接。 |
| HTTP Responses → WS 上游 | 优先 WS，保持客户端所需的 SSE 或 JSON 响应格式。HTTP background 请求使用 HTTP。 | 每请求独立，不保留给后续 HTTP 请求。 |

两条 WS 路径均使用 20 秒 Ping、10 秒 Pong 截止时间。复用前先做不生成内容的存活检查。心跳只检查连接是否存活；生成提交后不设置响应等待超时。

透明模式下，上游连接中断会结束客户端连接，由客户端重连并恢复各 lane。只在尚未提交生成时进行传输 fallback；写入可能部分成功、收到生成事件后断流等结果不明的情况不能自动重放。客户端主动关闭不应被归因为 provider 不支持 WS，也不应触发 provider 熔断。

## 会话降级与能力判定

原生 WS、HTTP → WS 与桥接共用握手分类。提交生成前连接失败会重连一次；仍无法传输时允许当前会话使用 HTTP/SSE。WS 会话以客户端连接为边界，HTTP 使用既有会话标识，缺少标识时仅影响本请求。新会话重新优先尝试 WS。临时错误不再触发 provider 级 WS 冷却。

| 证据 | 处理 |
| --- | --- |
| 超时、断连、无效升级、426、502/503 | 重连一次，必要时仅会话内降级。 |
| 401/403/429、参数或业务错误 | 保留错误及目标重试策略，不判定 WS 能力。 |
| 连续两次 404/405/501，随后同一 Responses 接口 HTTP 生成成功 | 确认自动关闭条件。HTTP 模型列表成功不能代替生成完成。 |
| 有效 WS 完成或 `generate=false` 空输出完成 | 确认支持，清除尚未成立的拒绝证据。单独 101 不足。 |

能力证据绑定 provider、凭据、接口、请求头和出站代理配置。配置切换使用新的判定版本，旧请求、并发成功及过期候选不能污染新判定；相同配置的元数据刷新保留证据。HTTP 会话标识最多保留 4096 个，30 分钟不用即过期。

agent 后台消费去重后的事件，在 Vault 中窄更新 `supports_websockets=false` 和自动关闭记录；锁库或失败时保留待处理状态，解锁后重新校验。配置已落盘但审计或刷新失败时，单独保留刷新重试，即使后来的 WS 成功清除了实时证据也不丢失。自动关闭不会截断正在返回的降级响应。保存后通过已有 revision 刷新桌面，重启和后续会话继续使用 HTTP/SSE。

详情状态区和编辑开关旁持续显示“该供应商可能不支持 WS，已自动关闭优先使用 WebSocket。”普通编辑不回传未触碰的旧开关值。恢复开启由 agent 使用待保存配置进行无生成探测：只有有效的空输出 `response.completed` 才保存开启并清除警告。失败、超时或配置并发变化时保持关闭及编辑草稿。网络验证不持有 Vault 锁，写回前再次比对配置。修改接口使旧证据失效，但保留自动关闭原因，仍须验证后开启。

## 跨 provider 的增量请求

桥接器只保存本连接各 lane 最近一次完成响应的完整输入和输出，以进程内可清零缓冲区保存，合计最多 64 MiB。客户端看到的是本地响应 ID；另一个 provider 不会收到前一个 provider 的 `previous_response_id`。

例如，A 返回 `function_call(call_1)` 后，Codex 只发送 `function_call_output(call_1)`。切换到 B 时，请求包含原用户输入、A 的调用定义和 Codex 的结果，保留当前请求的工具定义及参数。孤立的工具结果在发送前返回 `previous_response_not_found`，要求重送完整输入。

加密推理、压缩内容、文件 ID、服务端条目引用具有上游归属，不能当作普通文本任意迁移。历史中出现此类状态后，将响应链固定到产生该状态的 target；客户端续接时带回的状态沿用父响应的来源，工具或参数变化而重发完整输入时保留当前 lane 的来源。不删除这些字段来伪造成功。桥接连接尚无来源记录时，拒绝含此类状态的请求并返回 `previous_response_not_found`，要求使用原生模式或可迁移的完整输入。

这里绑定的是 AIPass target，不能保证上游网关内部仍选择同一账号；服务端条目也可能随上游连接过期而失效。服务端会话、自动压缩和生成中途 steering 仍要求原生透明路径。目标或隐藏状态不可用时需要客户端重建上下文，不能承诺任意加密状态可恢复。

跨 lane fork 与同 lane 续接遵循不同的缓存淘汰规则；失败的 fork 不删除来源 lane 的父响应。客户端关闭后，本地响应 ID、历史与上游连接同时失效。

## 协议与参考实现

- [OpenAI WebSocket mode](https://developers.openai.com/api/docs/guides/websocket-mode)：`previous_response_id` 的快速续接依赖连接内缓存；`store=false` 时缓存丢失没有持久化回退。重连后应使用完整输入开始新链。连接最多持续 60 分钟，不能承诺永久在线。文档也定义了 `stream_id` 的顺序、并发和 fork 语义。
- [Codex 客户端](https://github.com/openai/codex/blob/main/codex-rs/core/src/client.rs)：`responses_request_properties_match` 和 `get_incremental_items` 要求工具与其他非输入参数相同、输入能延续已知历史才发送增量。`ResponseCreateWsRequest::from(&request)` 保留当前请求参数，因此桥接器也必须保留当前工具定义。
- [New API 路由](https://github.com/QuantumNous/new-api/blob/0c76e4dae77a279e015329b7478e6f02d6b62edd/router/relay-router.go#L75)：本次检查的主线 WS 入口为 `/v1/realtime`，不能据此推断其支持 Codex Responses WS。[其 WS 会话处理](https://github.com/QuantumNous/new-api/blob/0c76e4dae77a279e015329b7478e6f02d6b62edd/relay/websocket.go)在会话结束时关闭上游。
- [sub2api 透明转发](https://github.com/Wei-Shaw/sub2api/blob/ab99d56e9626e6cd731592dae8553c9758a0efa2/backend/internal/service/openai_ws_v2/passthrough_relay.go)：分别跟踪客户端和上游退出，并关闭会话上游。[生命周期回归](https://github.com/Wei-Shaw/sub2api/blob/ab99d56e9626e6cd731592dae8553c9758a0efa2/backend/internal/service/openai_ws_v2_passthrough_lifecycle_test.go)覆盖关闭、超时和晚续接不能重放等情况；[续接测试](https://github.com/Wei-Shaw/sub2api/blob/ab99d56e9626e6cd731592dae8553c9758a0efa2/backend/internal/service/openai_ws_forwarder_ingress_session_test.go)区分完整工具历史可恢复与孤立工具结果不能换连接重试。

参考项目只用于核对行为与边界，未将其代码引入 AIPass。实际 provider 及其部署版本的能力仍由握手、生成结果和诊断日志确认。

## 回归覆盖

`crates/aipass-proxy/src/websocket/tests/adaptive.rs` 覆盖混合路由的原生透传、fallback 连接复用、空闲心跳、失效连接检查、客户端空闲/生成中关闭、跨 provider 的工具增量恢复、孤立工具结果拒绝和加密状态归属。原有 WS 测试覆盖帧透传、lane 并发与 fork、配置刷新、协议转换及提交后不重放。

能力回归还覆盖首次失败重连、HTTP 会话降级边界、明确拒绝联合判定、配置版本隔离、并发成功、偏好刷新不关连接，以及 agent 锁库/停止后持久化、Vault 重开和恢复探测的成功/失败/配置竞争。桌面回归检查后台关闭不覆盖其他编辑、失败时保留草稿和状态提示。

本地验证仅在 macOS 执行；按根目录 `agents.md` 的永久规则，不创建或运行 Ubuntu/Linux 测试容器。
