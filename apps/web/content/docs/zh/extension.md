---
title: 浏览器扩展
description: 安装 Chrome 扩展、与桌面应用配对，以及填充授权流程。
navTitle: 扩展
order: 6
---

# 浏览器扩展

Chromium 扩展（Chrome 和 Microsoft Edge）可以将 API 密钥填充到 AI 服务商控制台，而密钥从不会存储在浏览器中。它通过 Native Messaging Host 与 AIPass 代理通信，每次填充都由已解锁保险库发出的短时授权来许可。

## 安装与配对

1. 从 Chrome 应用商店或 Microsoft Edge 加载项安装扩展，或从 [GitHub Releases](https://github.com/backrunner/aipass/releases) 下载扩展包，在 `chrome://extensions` 或 `edge://extensions` 开启开发者模式后加载。
2. 注册 Native Messaging Host，让浏览器允许配对：

```bash
aipass native-host install --browser edge --extension-id <edge-extension-id>
```

Host 名为 `dev.aipass.native`。安装器写入清单（macOS 上为 `~/Library/Application Support/Google/Chrome/NativeMessagingHosts/dev.aipass.native.json`），其中 `allowed_origins` 被限制为你的扩展 ID，并保存 Native Host 在运行时强制执行的白名单。Chromium、Edge 和 Brave 可通过 `--browser` 使用。桌面应用会显示 Native Host 状态，并可在扩展设置中修复清单；`aipass doctor` 报告相同的检查项。

如果扩展无法连接保险库，按顺序检查：桌面应用已安装、代理正在运行（`aipass agent status`）、保险库已解锁、清单存在（`aipass doctor`）。

## 填充流程

当你打开已识别的服务商控制台——根据记录上的域名和控制台 URL 匹配——流程如下：

1. 扩展将页面来源（origin）的上下文查询发送给 Native Host。
2. 如果保险库**已解锁**，代理返回最多 5 条匹配记录，并为每个存储的密钥签发一个授权，每个授权绑定该来源，有效期 **120 秒**。
3. 弹出窗口列出匹配的记录。选择一条并点击填充。
4. 填充时扩展会请求一个新的授权，代理将其一次性消费，密钥被填充进页面（或通过剪贴板桥复制）。

授权是单一用途（`chrome.fill`）、绑定来源的，120 秒后过期；过期授权会被加密擦除——包裹的密钥材料会从授权文件中剥离。如果授权在你点击前过期，弹窗会向代理请求新的授权。保险库锁定时，弹窗可以触发解锁流程。

如果控制台页面没有显示任何记录，检查该记录的域名或控制台 URL 是否覆盖页面来源，或使用弹窗搜索找到任意记录并手动填充。

## 识别并保存新密钥

扩展会扫描服务商控制台页面中你创建或查看的 API 密钥。发现密钥时，弹窗会提供预填的草稿——标题、端点、接口、认证方式、标签——你确认后直接保存进保险库。在你确认之前不会有任何数据被发送，密钥最终以与桌面应用或 CLI 添加的记录相同的加密信封格式存储。

## 忽略来源

在弹窗中你可以忽略当前来源。被忽略的网站会记录在保险库中，不再触发查询或识别提示——适用于恰好匹配服务商域名但你永远不想填充的页面。

## 扩展做不到的事

- 它永远看不到你的主密码；解锁只能在桌面应用的原生界面或 CLI 中完成。
- 保险库锁定时，它无法列出记录、显示密钥或修改保险库。
- 它只能通过你安装时加入白名单的扩展 ID 与 Chrome 通信。
