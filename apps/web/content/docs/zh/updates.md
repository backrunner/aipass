---
title: 更新
description: 官方、Beta 与 Nightly 渠道、签名更新及重启行为。
navTitle: 更新
order: 8
---

# 更新

AIPass 通过 GitHub Releases 分发。**设置 → 更新**显示当前版本、渠道和**立即检查**。发现更新后，可在可用版本状态旁安装。

## 渠道

| 渠道 | Tag | 更新订阅源 |
| --- | --- | --- |
| 官方 | `vX.Y.Z` | `https://github.com/backrunner/aipass/releases/latest/download/latest.json` |
| Beta | `vX.Y.Z-beta.N` | `https://aipass.alkinum.io/api/updates/beta/latest.json` |
| Nightly | `vX.Y.Z-nightly.YYYYMMDD.N` | `https://aipass.alkinum.io/api/updates/nightly/latest.json` |

Nightly 最后的每日构建序号可省略，也支持 `vX.Y.Z-nightly.YYYYMMDD`。Beta 与 Nightly 订阅源按 tag 后缀选择已发布的对应预发布版本，不包含草稿或其他渠道的版本。订阅源返回该 release 的签名更新清单。

稳定版选择**官方**，预览版选择 **Beta**，跟进带日期的开发快照选择 **Nightly**。默认渠道跟随构建的版本后缀。手动选择按机器保存；安装另一渠道的构建时，选择可能重置为该构建的渠道。

## 安装更新

应用在后台检查所选订阅源，可提前下载更新。安装前会验证包签名。

准备好后选择**安装并重启**。推迟安装的下载包会缓存；后续启动时，若所选订阅源可访问且缓存仍有效，可以完成安装。下载失败或签名错误不会替换当前应用；使用**立即检查**查看错误并重试。

自动更新不安装更旧或相同的版本。同一基线版本内，预发布顺序为 beta、nightly，再到稳定版；因此切换渠道后，如果该渠道版本更旧，可能显示已是最新。

手动切换构建时，从 [GitHub Releases](https://github.com/backrunner/aipass/releases) 下载目标包并按[安装指南](/docs/zh/installation)操作。先阅读该版本的发布说明与保险库兼容性要求。

## 扩展包

Release 包含 `aipass-edge-extension.zip`。浏览器 manifest 使用纯数字构建版本，包元数据保留完整预发布版本。Nightly 扩展以 ZIP 分发，不作为 nightly 商店版本提交。

使用桌面应用的扩展设置与修复操作。桌面应用不会用解压版更新替换商店安装的扩展副本。

## 发布验证

发布需要完成签名、制品验证及启动和更新检查。发布后，维护者还会核对公开下载文件和对应渠道的更新源。流程与贡献者致谢见[开发与发布](/docs/zh/development#发布)。
