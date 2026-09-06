# Codex、AIPass 与上游的 WS 会话

检查日期：2026-09-07。连接生命周期以 Codex 客户端为边界。

## 转发规则

| 路径 | 请求与状态 | 连接生命周期 |
| --- | --- | --- |
| Codex WS → 原生 Responses WS | 选中上游后固定连接，应用帧原样转发，不改写工具、输入、响应 ID 或 `stream_id`。路由包含其他协议或开启转换选项，本身不触发转换。 | 一对一；客户端关闭时关闭上游，新客户端连接不复用旧连接。 |
| Codex WS → fallback/协议桥接 | 按 lane 排队、维护本连接内的响应链，将 `previous_response_id` 与新输入恢复为完整输入，再选择 provider。 | 只在当前客户端连接内保留成功的 WS 上游，空闲 120 秒过期，最多 32 个；客户端关闭或配置刷新时全部清理。 |
| HTTP Responses → WS 上游 | 优先 WS，保持客户端所需的 SSE 或 JSON 响应格式。HTTP background 请求使用 HTTP。 | 每请求独立，不保留给后续 HTTP 请求。 |

两条 WS 路径均使用 20 秒 Ping、10 秒 Pong 截止时间。复用前先做不生成内容的存活检查。心跳不算生成进展，不延长 pending response 的超时预算。

透明模式下，上游连接中断会结束客户端连接，由客户端重连并恢复各 lane。只在尚未提交生成时进行传输 fallback；写入可能部分成功、收到生成事件后断流等结果不明的情况不能自动重放。客户端主动关闭不应被归因为 provider 不支持 WS，也不应触发 provider 熔断。

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

本地验证仅在 macOS 执行；按根目录 `agents.md` 的永久规则，不创建或运行 Ubuntu/Linux 测试容器。
