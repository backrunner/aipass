# 一键导入本机订阅账号

解锁密码库后，在“连接订阅”或账号列表点击“一键导入本机账号”。
Agent 立即扫描已知位置并逐项连接，结果弹窗显示账号、来源和处理状态。
启动、解锁不会自动扫描；本机订阅导入无需打开 CC Switch 设置。

支持 Claude、Codex、Grok Build、GitHub Copilot、Gemini CLI、ZCode、Devin、
Command Code、Cursor、Kiro、WorkBuddy 国内和国际账号。
Qoder 国内／国际、Zed、Factory、MiMo 继续使用原有登录入口。
没有本机登录文件的网页账号也需要单独登录。

五类官方 CLI 检查默认位置、环境变量指定位置、AIPass 独立账号目录的直接
子目录，以及当前设备已有绑定。其他服务商检查现有适配器支持的本机存储。
Kiro 分别枚举 CLI social、OIDC、external IdP 和 IDE；ZCode 分别保留站点、
组织和项目范围；WorkBuddy 两个区域分别建账。Cursor 安全存储与文件来源
各自报告结果，不会用文件账号掩盖安全存储读取失败。

## 自定义目录和恢复

在结果弹窗选择服务商，再点击“选择账号目录”，补选后直接导入。
选择包含登录文件的目录；Gemini CLI 应选择包含 `.gemini/` 的 home 根目录。
来源规范化后去重，扫描不会递归搜索整个用户目录。

- “已导入／已更新”：账号已保存，可以通过现有流程配置代理分组和优先级。
- “已存在”：保留已有有效绑定，不重写凭据或旋转 generation。
- “需要登录”：通过该项的登录入口恢复，再重试失败项。
- “没有发现账号”：正常结果，尚未安装或没有本机登录文件。
- “读取失败”：检查权限、格式或安全存储授权；可重试或补选目录。
- “已取消”：未完成项停止，已导入账号继续保留。

任务运行期间可取消。关闭弹窗保留任务和已完成账号；再次点击账号列表的
导入按钮可查看进度，不会重复启动。锁库或切换密码库会取消旧任务，重新
解锁不会恢复旧任务的写入权限。完成结果在 Agent 内存保留五分钟。
CC Switch API 配置通过设置中的独立入口导入，保留检测和冲突提示。

## CLI

```sh
aipass accounts import
aipass accounts import --provider codex --provider gemini-cli
aipass accounts import --provider gemini-cli --directory /absolute/gemini-home
aipass --json accounts import --provider kiro
```

`--directory` 要求恰好一个 `--provider`，使用绝对路径。
普通输出逐项显示结果，JSON 输出提供与桌面相同的任务模型。
Ctrl-C 请求取消并等待最终结果。`accounts refresh` 保留原来的官方 CLI
过滤语义和结果格式，包括忽略不支持的服务商过滤项。

## Rust 和 IPC 边界

协议版本 16 提供 `subscription.import.start`、`subscription.import.poll` 和
`subscription.import.cancel`。Start 接收 `providerIds`、`sources` 和可选
`retry: {ticket, sourceIds}`；Poll/Cancel 接收 `ticket`。每项结果只包含来源、
身份、状态、entry ID、错误码和恢复动作，不包含 token。

每个解锁会话只运行一个任务，最多并发读取四个来源，每个来源限时 60 秒。
发现和身份查询不持有 vault 锁，提交逐账号原子执行并重新验证会话及账号
revision。失败项互不阻塞；每次提交后刷新受影响的运行中代理凭据。
导入不启动浏览器登录、不调用模型生成、不主动消耗 refresh grant。

Claude、Codex、Grok Build、Copilot 和 Gemini CLI 只保存设备绑定引用。
其他七类复用现有加密账号格式，保存明确来源，并在后续读取和续期时使用
该来源；跨设备使用需要在目标设备重新连接。重复账号按规范服务商和稳定
身份去重，保留已有有效绑定，补选来源优先于默认位置。绑定改变保留 entry
和 secret ID。旧 ZCode 用户身份只升级匹配的站点／组织／项目，native method
记录实际值 `2`。

## 验证边界

Fixture 回归覆盖来源枚举、作用域隔离、重复导入、稳定 ID、加密输出、
失败项重试、取消及会话失效。桌面测试覆盖两个入口共享任务、补选目录和
恢复动作；960×640 的 WebKit fixture 覆盖中英文、明暗主题和长路径列表。
这些结果不代表所有服务商真实订阅账号已完成验收；真实账号需按实际可用
来源分别记录导入、身份及来源绑定证据。
