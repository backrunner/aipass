# CloudKit 发布设置

默认容器为 `iCloud.com.alkinum.aipass`，App ID 为 `com.alkinum.aipass`。签名的 macOS 桌面 App 使用 CloudKit 私有数据库；Agent、CLI、浏览器与 WebView 不直接访问 CloudKit。

## Apple Developer 与 CloudKit Console

1. 为 App ID 启用 iCloud（CloudKit）和 Push Notifications，关联上述容器。
2. 为相同 Team / App ID 生成 **Developer ID provisioning profile**，包含 CloudKit Production 和 production APNs 权限。现有 Developer ID 证书本身不等于具备 CloudKit 权限。
3. 使用下面的 CLI 脚本将 `infra/cloudkit/schema.ckdb` 部署到开发和生产环境。`VaultSnapshot.ciphertext` 类型为 **Asset**，只授予 creator 读写权限。应用自动创建私有 custom zone `AIPassVault`，recordName 为密文 SHA-256，订阅 ID 为 `aipass-vault-changes-v1`。zone change API 不依赖可查询的自定义索引。
4. 使用签名且嵌入 profile 的正式候选包，在同一 Apple 账户的两台 macOS 设备验证上传、首次安装恢复、后台推送和断网重试，再验证更换 Apple 账户会停止该 vault 的同步。开发和生产数据库彼此独立。

这些账户和服务端步骤没有由本地构建自动完成。当前工作区未提供 AIPass CloudKit provisioning profile。2026-09-07 本机 `cktool get-teams` 可识别团队 `PB8H83VL3Z`，但默认容器的开发、生产 schema 导出均返回 `authorization-failed`；部署未执行，需确认容器已创建且保存的管理令牌具有该容器权限。跨设备验收尚未完成。

## CLI 部署

先在 CloudKit Console 获取具有目标容器权限的管理令牌，用 `xcrun cktool save-token --type management` 的安全交互输入保存；不要将令牌加入命令行参数、源码或日志。`cktool` 不能创建尚不存在的容器，容器需先在 Apple Developer 账户中创建并关联 App ID。

```sh
DEVELOPER_DIR=/Applications/Xcode-beta.app/Contents/Developer \
  node scripts/deploy-cloudkit-schema.mjs --team PB8H83VL3Z --deploy
```

按实际安装路径设置 `DEVELOPER_DIR`，只影响此命令，无需修改全局 `xcode-select`。`--container`（或 `AIPASS_CLOUDKIT_CONTAINER`）覆盖默认容器；不传 `--deploy` 时只导出、生成候选与服务端验证。

脚本先导出两个环境，并保留全部已有 record type、字段和索引，只补齐 AIPass 定义。遇到已有不兼容字段或非 creator 权限时停止。两个环境均验证成功后，依次导入开发和生产，再重新导出核对。每次运行在独立临时目录保存前后 schema、候选文件与成功报告，也可用 `--output` 指定存放目录。任何一步失败都会返回非零状态；生产导入失败时可能已经完成开发导入，保留输出并在解决错误后重跑。脚本不调用 reset、删除记录或创建测试数据。

## 构建与发布

GitHub release workflow 新增必需 secret `APPLE_PROVISIONING_PROFILE_BASE64`（上述 `.provisionprofile` 的 base64）。继续使用现有 `APPLE_TEAM_ID`、Developer ID 签名证书与 notarization 凭据。

本地发布候选可设置 `APPLE_PROVISIONING_PROFILE` 指向该文件，随后运行 `scripts/prepare-tauri-macos-release-config.mjs`。脚本只解码、校验并生成构建输入，不修改 Apple 账户。它检查 Team、App ID、平台、有效期、Developer ID 类型、容器、CloudKit 与 production APNs 权限。

生成内容位于忽略目录 `apps/desktop/src-tauri/.cloudkit-build/`：

- `Entitlements.plist`：正式 App 所需 CloudKit/推送与 hardened runtime 权限。
- `embedded.provisionprofile`：Tauri 在签名前复制到 App 的 `Contents`。
- `Info.plist`：原生传输使用的容器 ID。

`AIPASS_CLOUDKIT_CONTAINER` 可覆盖默认容器（CI 对应同名 repository variable），但 profile 必须明确授权该容器。原生代码从 App Info.plist 读取同一个值。Agent/native-host sidecar 保留独立的基础签名，不使用 App 的 CloudKit 权限。

构建后 `scripts/verify-cloudkit-bundle.mjs` 校验嵌入的 profile 和实际签名中的权限；release workflow 在上传产物前运行该检查。缺少或不匹配的 profile 会阻止发布包生成，普通未签名 macOS bundle 检查不需要 profile。

## 本地验证边界

`scripts/cloudkit-profile.test.mjs` 验证生产签名输入和错误 profile 拒绝。Agent 测试用假原生传输验证密文恢复、账户绑定、重发和错误返回；桌面 Rust 测试调用 Swift FFI，确认无 entitlement 时立即返回不可用。真实 CloudKit/APNs 的成功与时延必须由正式候选包验证，不能由这些模拟测试推断。
