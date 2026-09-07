# 本地代理 Images API

本地代理支持 `POST /v1/images/generations` 和 `POST /v1/images/edits`。
请求可使用 JSON，编辑上传也可使用 multipart/form-data。原始请求字节、
multipart boundary、上游图片结果和 SSE 预览事件保持透传；API base、
query、凭据替换及客户端身份头复用普通代理规则。

OpenAI Responses 或 Chat Completions 路由的本地 token 可访问这两个接口，
只会选择该路由内启用的 OpenAI targets。Anthropic targets 不参与，
Images 请求不进行聊天协议转换，也不修改 Responses 会话粘性。
Codex 的 Responses token 仍不能访问 Chat Completions 或 Anthropic 会话接口。

## 客户端与上游的职责

- Codex 或其工具直接使用 Images API 时，请求的 base URL 和凭据必须指向
  AIPass 本地代理。仅配置 Codex 的 model provider，不保证外部脚本、SDK、
  MCP 自动继承相同的 API 地址。
- Responses 原生 `image_generation` 工具由上游执行，沿 `/v1/responses`
  返回图片结果；它不会要求 AIPass 再调用一次 `/images/generations`。
- 本次 Images 能力记录不代表 Responses 图片工具支持，也不实现 Codex
  `image_gen` namespace 到原生工具的桥接。

## 从真实请求学习能力

不发送额外探测请求。每个操作、模型、请求 streaming 模式与有效 provider
配置分别记录未知、已验证支持或明确不支持。配置身份涵盖 endpoint、
凭据、provider headers 和出口代理；不会仅凭品牌、模型名称或模型列表判断。

- 完整 Images JSON 图片结果或相应 SSE completed 图片结果确认支持。
- 明确的 endpoint/model/image capability 不支持错误可确认当前组合不支持。
- 普通 404、权限、额度、限流、图片参数错误和没有生成结果不构成不支持证据。
- 排除已知不支持者及非 OpenAI targets 后，复用健康筛选；优先选择已验证支持者，
  然后应用尝试次数限制。未知目标可以承接真实任务。
- 所有候选不可用时返回 503，不启动额外探测。

能力表最多 4,096 条，只在代理运行内存中保留，停止/锁定后重新学习。
不支持记录有效期 1 小时，支持记录有效期 24 小时。过期只允许未来真实请求
重新验证，不触发后台请求。新配置与旧配置的证据相互隔离；较旧在途请求
不能覆盖更新请求的结论。

## 流、错误和诊断

图片 SSE 预览及时转发，观察器支持跨网络块的大图片事件，单个观察事件及
非流式响应上限为 64 MiB。上传沿用 512 MiB 请求上限，大于 8 MiB 时复用
代理的临时文件缓冲；multipart 只提取 model/stream 字段，文件内容不重编码。

仅连接失败或明确的执行前拒绝允许尝试后续目标。图片流提交后、连接丢失或
结果截断时不重放；普通上游 5xx 也按执行情况未知处理。Images 每次请求最多
使用路由的 max_attempts 个目标，不启动 silent retry/hold 的重复轮次。
提交后不使用首字节、空闲或 hold 时限终止慢生成；客户端断开或配置撤销会取消。

`proxy.images.routing` 记录可用候选数，`proxy.images.forwarding` 记录实际目标，
`proxy.images.capability` 记录操作与支持状态。上游拒绝沿用脱敏错误日志，
最终请求/尝试沿用既有用量记录。能力诊断不记录提示词、图片、凭据或模型名称。
现有用量记录仍按路由协议归档；本实现不把图片费用套用文本模型价格，
图片 token/费用字段暂记为 0，实际扣费以上游为准。
