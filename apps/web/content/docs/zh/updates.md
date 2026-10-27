---
title: 更新
description: 官方、Beta 与 Nightly 渠道、签名更新及重启行为。
navTitle: 更新
order: 8
---

# 更新

AIPass 通过 GitHub Releases 分发。**设置 → 更新**显示当前版本、渠道和**立即检查**。发现更新后，可在可用版本状态旁安装。

## 渠道

| 渠道    | Tag                         | 更新订阅源                                                                  |
| ------- | --------------------------- | --------------------------------------------------------------------------- |
| 官方    | `vX.Y.Z`                    | `https://github.com/backrunner/aipass/releases/latest/download/latest.json` |
| Beta    | `vX.Y.Z-beta.N`             | `https://aipass.alkinum.io/api/updates/beta/latest.json`                    |
| Nightly | `vX.Y.Z-nightly.YYYYMMDD.N` | `https://aipass.alkinum.io/api/updates/nightly/latest.json`                 |

Nightly 最后的每日构建序号可省略，也支持 `vX.Y.Z-nightly.YYYYMMDD`。Beta 与 Nightly 订阅源按 tag 后缀选择已发布的对应预发布版本，不包含草稿或其他渠道的版本。订阅源返回该 release 的签名更新清单。

稳定版选择**官方**，预览版选择 **Beta**，跟进带日期的开发快照选择 **Nightly**。默认渠道跟随构建的版本后缀。手动选择按机器保存；安装另一渠道的构建时，选择可能重置为该构建的渠道。

## 安装更新

应用在后台检查所选订阅源，可提前下载更新。安装前会验证包签名。

准备好后选择**安装并重启**。推迟安装的下载包会缓存；后续启动时，若所选订阅源可访问且缓存仍有效，可以完成安装。下载失败或签名错误不会替换当前应用；使用**立即检查**查看错误并重试。

自动更新不安装更旧或相同的版本。同一基线版本内，预发布顺序为 beta、nightly，再到稳定版；因此切换渠道后，如果该渠道版本更旧，可能显示已是最新。

手动切换构建时，从 [GitHub Releases](https://github.com/backrunner/aipass/releases) 下载目标包并按[安装指南](/docs/zh/installation)操作。先阅读该版本的发布说明与保险库兼容性要求。

## CLI 更新

```bash
aipass self-update check --json
aipass self-update install --yes --json
aipass self-update check --channel nightly --target cli --json
aipass self-update install --channel beta --target desktop --yes --json
```

`--target auto` 自动识别包含三个可执行文件的独立安装目录或桌面配套 CLI，也可指定 `cli` 或 `desktop`。默认渠道跟随 CLI 构建；`--channel official|beta|nightly` 仅影响本次操作，不修改桌面偏好。原有 `aipass update <id>` 仍用于编辑凭据。

检查不会安装、解锁保险库或询问密码。桌面操作会在需要时启动已安装的 AIPass 托盘实例。安装必须传入 `--yes`，支持重定向标准输入，并重启对应运行时。Agent 重启会结束当前解锁会话。没有更新时成功退出，不安装更旧版本。

桌面更新复用签名更新器，确认新桌面进程达到目标版本、CLI 已刷新且 Agent 能响应后，才返回安装成功。重启后保持托盘模式。AppImage 支持无人值守更新；DEB/RPM 请使用系统包管理器，避免触发授权窗口。权限不足时直接报错，不请求提权。

独立更新从官方 GitHub 仓库读取发布元数据，要求发布文件包含 SHA-256 摘要，校验下载内容与可执行文件架构，再一起替换 CLI、Agent 和 Native Host。摘要缺失、架构不匹配、包格式或版本错误都会在替换前失败。替换失败会恢复旧文件；恢复无法完成时保留恢复文件。三个可执行文件必须放在同一可写目录。更新前请停止其他保险库使用此安装目录的 Agent。

`--timeout-seconds 300` 限制每次网络操作及桌面重启等待时间，范围为 1–3600 秒。JSON 模式的标准输出包含一个结果对象。退出码 `0` 表示检查或安装完成，包含“已是最新”；`1` 表示操作失败；`2` 表示参数错误，包含安装时缺少 `--yes`。检查结果使用 `available` 和 `latestVersion`，安装成功返回 `status: "installed"`。

## 扩展包

Release 包含 `aipass-edge-extension.zip`。浏览器 manifest 使用纯数字构建版本，包元数据保留完整预发布版本。Nightly 扩展以 ZIP 分发，不作为 nightly 商店版本提交。

使用桌面应用的扩展设置与修复操作。桌面应用不会用解压版更新替换商店安装的扩展副本。

## 发布验证

发布需要完成签名、制品验证及启动和更新检查。发布后，维护者还会核对公开下载文件和对应渠道的更新源。流程与贡献者致谢见[开发与发布](/docs/zh/development#发布)。
