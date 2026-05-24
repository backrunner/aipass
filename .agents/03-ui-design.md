# AIPass UI/UX 设计文档

设计目标：像 1Password 一样可靠、克制、快速，但更懂 AI Provider。AIPass 不做“AI 感”的花哨界面，不堆叠卡片和解释文本；它应该像一个好用的开发者工具，安静、精密、可快速操作。

## 1. 设计原则

- 低噪音：默认只展示用户作业所需信息，说明文字进入 tooltip、empty state 或 docs。
- 高密度但不拥挤：列表和详情适合频繁扫描，不做大面积营销 hero。
- 可预测：三栏结构、明确焦点、可键盘操作、状态可见。
- 安全有摩擦：复制、Reveal、填充、写配置有轻量确认和审计，不静默泄露。
- Provider-first：域名、favicon、平台类型、endpoint、接口协议、认证方式是主要视觉线索。
- Local-first：同步是状态提示，不喧宾夺主。

## 2. 视觉方向

### Tone

“Quiet Developer Vault”：冷静、清晰、略带精密感。参考 1Password 的工作台效率，但更偏 developer console。

### 颜色

避免大面积紫蓝渐变。建议使用多中性色 + 少量功能色：

- Background: `#F7F7F4` light / `#111312` dark
- Surface: `#FFFFFF` light / `#181B1A` dark
- Sidebar: `#ECEDE8` light / `#151716` dark
- Text primary: `#171A18` light / `#F2F3EE` dark
- Text secondary: `#66706A` light / `#A5ADA7` dark
- Accent: `#2D7D6B` teal
- Official: `#2563EB` blue
- Third-party: `#9A6A1E` amber
- Self-hosted: `#6B7280` neutral
- Danger: `#B42318`
- Success: `#18794E`

### Typography

- Desktop UI 使用系统字体栈，优先平台原生清晰度。这里允许系统字体，因为这是生产工具，不是品牌展示页。
- 数字、fingerprint、quota 使用 tabular figures。
- Secret preview 使用等宽字体，但只显示 masked + suffix。

### Radius & Elevation

- 控件 radius 6px，卡片/面板最多 8px。
- 避免卡片套卡片。
- 主布局用分栏和边界线，不使用大阴影。
- Popover/Dialog 使用轻微 elevation，明确层级。

## 3. 信息架构

```text
Unlock
Main Workspace
  Sidebar
    All Items
    Favorites
    Recent
    Official Providers
    Third-party Platforms
    Self-hosted Gateways
    Tags
    Archive
  List
    Search
    Filter chips
    Entry rows
  Detail
    Header
    Secret fields
    Endpoint fields
    Tool config actions
    Quota
    Activity
Settings
  Security
  Sync
  Browser Extension
  CLI Integrations
  Provider Registry
  About / License
Command Palette
Quick Add
Conflict Resolver
```

## 4. 桌面端布局

### 主工作台

三栏尺寸：

- Sidebar: 224px，最小 192px，最大 280px。
- List: 360px，最小 300px，最大 480px。
- Detail: flex 1，最小 420px。

小屏策略：

- `< 900px`: Sidebar collapsible，List + Detail 双栏。
- `< 680px`: 单栏 drill-in，List 与 Detail 通过 navigation stack 切换。

### Sidebar

内容：

- 顶部：Vault selector + lock status。
- 一级入口：All Items、Favorites、Recent。
- 分组：Official、Third-party、Self-hosted、Unknown。
- Tags 折叠展示，显示数量。
- 底部：Sync status、Settings。

交互：

- 当前项用细左边线 + 背景 tint，不使用大 pill。
- 分类数量右对齐。
- Sync error 显示小红点，点击进入 settings/sync。

### List

Entry row:

- favicon/initial icon
- title
- domain/endpoint
- provider badge
- masked key suffix，如 `•••• 8K2Q`
- quota/status mini indicator
- last used relative time

List toolbar:

- Search input 始终可见。
- Filter button 使用 icon + popover。
- Add button 使用 plus icon。

空状态：

- 无条目：只给一个主动作 `Add Provider` 和一个次动作 `Import`。
- 无搜索结果：显示 search token 和 `Create custom provider`。

### Detail

Header:

- favicon + title + provider type badge。
- 主动作：Copy Key、Configure、Reveal menu。
- 次动作：Edit、More。

Sections:

- Secret: API key、额外 headers、organization/project id。
- Endpoint & Interface: endpoint/base URL、console URL、接口协议、认证方式、default model、models aliases。
- Tool setup: Codex、Claude Code、Gemini CLI quick configure。
- Quota: 限额、到期日、使用备注。
- Activity: 最近复制/填充/配置写入，不显示明文。

Secret field:

- 默认 masked。
- Copy icon button、Reveal hold-to-show 或 click with timeout。
- Copy 后显示 2s check 状态，并可提示剪贴板将在 N 秒后清理。

## 5. 创建/编辑流程

### Quick Add

入口：顶部 plus、command palette、extension save prompt。

步骤：

1. 选择或自动识别 provider。
2. 填写 title、domain/endpoint、API key、认证方式和接口协议。
3. 展开高级项：headers、models、quota、tags。
4. 保存后进入 detail。

原则：

- 默认一屏完成。
- 高级项折叠。
- API key 输入框提供 paste 检测和 fingerprint 预览。
- 如果 endpoint 或页面特征看起来是 new-api/one-api/LiteLLM，自动切换为 self-hosted gateway template；如果是 Anthropic/Gemini/Azure/Bedrock，则保留各自原生字段。

### Provider Wizard

用于未知 endpoint：

- 输入 endpoint/base URL 或 console URL。
- 可选点击 Test Endpoint。
- AIPass 根据 provider 类型调用 provider-specific probe；OpenAI-compatible 可调用 `/v1/models`，Anthropic/Gemini/Azure/Bedrock 使用各自探测策略。
- 根据响应推断接口协议和认证方式。
- 让用户确认 provider kind 和默认模型。

## 6. Chrome 扩展体验

### Popup

尺寸：360x520。

状态：

- Locked: AIPass logo、Unlock button、connection status。
- Current site matched: 显示当前域名 entries，支持 copy/fill。
- No match: `Save new provider`、`Search AIPass`。
- Save detected key: 显示被检测内容的安全摘要。

不显示长文本说明。所有解释进入 tooltip 或 detail line。

### Inline suggestion

在 API key 创建/复制区域旁显示小型 AIPass affordance：

- `Save in AIPass`
- `Use existing key`
- `Ignore this site`

安全要求：

- 只有用户点击或页面出现明确 key-created/copy UI 后才读取字段。
- 检测到的 key 只在 content script memory 中短暂保存，并尽快交给 native host。
- 保存前必须展示确认。

## 7. CLI 体验

CLI 文案要短而确定：

```text
$ aipass configure codex openai-prod
Target: ~/.codex/config.toml
Mode: helper
Provider: OpenAI Prod

Changes:
  + [model_providers.aipass_openai_prod]
  + env_key = "AIPASS_OPENAI_PROD"
  + api_key_helper = "aipass get ..."

Apply? [y/N]
```

原则：

- `doctor --json` 可被自动化使用。
- 写配置前有 diff。
- 成功后给下一步命令。
- 错误信息包含修复动作，不暴露 secret。

## 8. 组件规范

优先 Bits UI：

- Dialog: 创建/编辑/确认/冲突解决。
- Popover: filter、more actions、copy variants。
- DropdownMenu: entry actions。
- Select/Combobox: provider、model、environment。
- Tabs: Settings 子页面。
- Switch/Checkbox: sync/autolock/extension permissions。
- Slider/Number input: auto-lock timeout、clipboard clear timeout。
- Tooltip: icon buttons。

Radix 作为交互参考：

- Focus trap。
- Escape 关闭。
- Arrow key navigation。
- Roving tabindex。
- `data-state` styling。

自定义组件：

- `ProviderIcon`
- `SecretField`
- `EndpointField`
- `QuotaMeter`
- `ProviderBadge`
- `ToolConfigButton`
- `SyncStatusPill`
- `CommandPalette`
- `EntryListRow`

## 9. 可访问性

- 所有 icon-only button 必须有 `aria-label` 和 tooltip。
- Reveal secret 后 screen reader 不自动朗读 secret。
- Copy secret 操作要有 live region 状态，但不读出 secret。
- 键盘顺序：sidebar → search → list → detail actions → detail fields。
- 支持 high contrast mode。
- 支持 reduced motion，禁用非必要动画。

## 10. 动效

- 页面切换 120-180ms opacity/translate。
- Dialog 160ms scale/opacity。
- Row hover 80ms background。
- Copy success 120ms icon morph。
- 不做装饰性循环动画。

## 11. 关键状态

- Locked: 清晰、克制，只显示 unlock。
- Offline: 同步状态变为灰色，不影响本地使用。
- Sync conflict: Sidebar 底部小红点 + conflict view。
- Secret revealed: 字段背景轻微 warning tint，倒计时可见。
- Extension connected: Settings 中显示 extension id 和最后连接时间。
- Native host missing: 扩展显示安装/修复引导。

## 12. 视觉验收

- 主界面第一屏无需滚动即可完成搜索、选择、复制。
- 任何一屏只有一个主动作。
- 1,000 条列表仍能扫描，不被卡片化设计拖慢。
- 文案不解释功能本身，只标注对象和动作。
- 在 1280x800、1440x900、390x844 三种尺寸无重叠和截断。
