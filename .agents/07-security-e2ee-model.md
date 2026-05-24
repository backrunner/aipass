# AIPass E2EE 安全模型

本文是 AIPass 的 P0 安全约束。AIPass 保存的是 AI Provider API Key，一旦本地文件、同步目录或备份文件可被直接读出 secrets，产品就不成立。

## 1. P0 Security Invariant

任何人只拿到 AIPass 的持久化文件时，不能恢复任何 AI secret。

这里的“持久化文件”包括：

- 本地 vault 目录。
- iCloud/WebDAV/本地文件夹同步目录。
- 导出的备份文件。
- 搜索索引。
- 审计日志。
- 配置写入 backup。
- crash/log/diagnostic bundle。
- Chrome extension storage。
- Native host manifest 与本地 agent 配置。

只有同时具备用户的 master password，或用户明确授权的已解锁设备会话，才能解密 API secrets。

## 2. Threat Model

### 必须抵抗

- 攻击者复制整个 `.aipass` vault 目录。
- 攻击者拿到 iCloud Drive/WebDAV 远端同步文件。
- 攻击者拿到用户导出的 backup。
- 攻击者读取搜索索引、审计日志、应用日志。
- 攻击者拿到未解锁状态下的电脑磁盘镜像。
- 不可信 WebDAV 服务端试图读取或篡改同步对象。
- 浏览器页面试图诱导扩展泄露已保存 key。

### 不承诺完全抵抗

- 设备已经被恶意软件控制且 vault 当前处于解锁状态。
- 用户主动 Reveal/Copy 后粘贴到不可信位置。
- 用户选择明文导出或明文写入第三方 CLI 配置。
- 低熵 master password 被离线暴力破解。AIPass 必须通过 KDF 和密码强度提示降低风险，但不能消除弱密码风险。

## 3. Encryption Design

### Key Hierarchy

```text
Master Password
  -> Argon2id(password, vault_salt, kdf_params)
    -> Master Key
      -> decrypt Key Encryption Key envelope
        -> Vault Key
          -> decrypt record keys or record payloads
            -> Provider entries and API secrets
```

可选设备解锁：

```text
Vault Key
  -> wrapped by Device Key
    -> stored in OS secure storage / Tauri Stronghold
```

设备解锁只是便利层，不是唯一恢复方式；用户仍必须拥有 master password 或 recovery material。

### Forward Security Boundary

AIPass 需要前向安全思路，但必须区分三类数据：

- Active provider entries: 用户未来仍要使用的 API 配置，必须可恢复。因此它们不能通过简单删除旧 key 来“自动失效”，否则用户也读不回来。
- Ephemeral artifacts: 浏览器填充授权、CLI 临时 env session、Reveal token、旧版本草稿、同步 staging、过期分享。这些可以用 ratchet/TTL key，并在到期后通过删除 key 做 cryptographic erasure。
- Superseded history: 被编辑覆盖的旧 record version。默认不长期保留；如需保留历史，必须单独声明 retention policy。

重要边界：如果攻击者已经复制了文档密文和当时可解密它的 key，后续任何本地操作都不能让攻击者手里的旧副本“失效”。加密失效依赖 key destruction，只对仍受 AIPass 管控的 key store、生效中的会话和未来写入有效。

### Per-record DEK

每条 provider record 必须使用独立随机 Data Encryption Key：

```text
record_plaintext
  -> encrypt with random Record DEK
    -> record ciphertext
Record DEK
  -> wrap with Vault Epoch Key
    -> encrypted key envelope
```

禁止用同一个长期内容密钥直接加密所有 provider records。否则一个 key 泄露会解开整个 vault，而且无法选择性吊销单条记录。

### Vault Epoch Ratchet

AIPass 应维护 Vault Epoch Key：

```text
VEK_0 = random(32)
VEK_n+1 = HKDF(random(32) || HKDF(VEK_n, "advance"), context)
```

要求：

- 每次 epoch advance 都引入新的 OS CSPRNG 随机数。
- 成功 rewrap/re-encrypt 后从内存中擦除旧 VEK。
- 当前 VEK 不能推导旧 VEK。
- 旧 VEK 不能推导新 VEK，因为新 epoch 混入新随机数。
- suspected compromise、设备移除、master password 修改、provider key rotation 后必须触发 epoch advance。

这提供两个效果：

- Forward secrecy for erased past: 当前 epoch key 泄露时，无法推导已经擦除的旧 epoch key。
- Post-compromise recovery for future: 泄露发生后，只要攻击者不再持续控制设备，引入新随机数并 rekey 后，未来写入可重新安全。

### TTL / Cryptographic Erasure

对需要“多久后失效”的对象，不依赖 ciphertext 自己判断时间，而依赖 key lifecycle：

```text
Expiring object
  -> encrypt with Object DEK
Object DEK
  -> wrap with TTL Bucket Key or per-object expiry key
Expiry reached
  -> delete wrapped DEK / delete TTL key
  -> sync key-destruction tombstone
  -> ciphertext remains but cannot decrypt
```

适用对象：

- Reveal session。
- Browser extension fill grant。
- CLI temporary env grant。
- One-time export token。
- Old record versions。
- Sync conflict temporary payload。

不适合对象：

- 用户仍希望长期使用的 active API Provider 配置，除非到期后明确希望它不可恢复。

### Document Revocation Answer

“用同一个 key 加密文档 2，顺便吊销文档 1”在密码学上不成立。只要文档 1 和文档 2 共用同一个解密 key，保留这个 key 就仍能解密文档 1。

可行做法：

- 文档 1 和文档 2 使用不同 Record DEK。
- 文档 1 到期后删除文档 1 的 DEK 或 TTL wrapping key。
- 文档 2 使用新的 DEK，并由当前 Vault Epoch Key 包裹。
- 如果文档 1 必须从所有未来备份中失效，备份也必须执行 key deletion 或重建；已经被别人复制走且包含旧 key 的备份无法远程吊销。

### Algorithms

MVP 推荐：

- KDF: Argon2id，参数随设备能力校准，默认内存成本不低于 64 MiB，优先 128-256 MiB。
- AEAD: XChaCha20-Poly1305 或 AES-256-GCM。优先 XChaCha20-Poly1305 以降低 nonce 复用风险。
- Random: OS CSPRNG。
- Fingerprint: HMAC-SHA-256，key 来自 vault 内独立 index key，不使用裸 SHA。
- Memory: secret buffers 使用 zeroize，避免长期驻留。

### Envelope

每个密文对象必须包含：

```json
{
  "format": "aipass-object",
  "version": 1,
  "vault_id": "vlt_...",
  "object_id": "rec_...",
  "object_type": "provider_entry",
  "crypto": {
    "aead": "xchacha20poly1305",
    "nonce": "base64...",
    "aad_hash": "base64..."
  },
  "sync": {
    "device_id": "dev_...",
    "lamport": 42,
    "updated_at": "2026-05-24T00:00:00Z"
  },
  "ciphertext": "base64..."
}
```

AAD 必须覆盖：

- vault id
- object id
- object type
- schema version
- crypto version
- sync metadata 中会影响合并语义的字段

任何篡改都必须导致验证失败或进入 quarantine。

## 4. What May Be Plaintext

默认策略：除了无法避免的文件格式版本和对象路由字段，尽量不明文保存业务语义。

允许明文：

- vault format version。
- crypto algorithm id。
- KDF parameters 和 salt。
- object id、object type。
- sync lamport/device id/hash。
- 非敏感 app settings，例如 theme、窗口尺寸。

默认不允许明文：

- API key、token、secret headers。
- endpoint/base URL、console URL。
- provider title。
- domain。
- provider kind。
- tags。
- quota。
- notes。
- model aliases。
- tool config history 中可关联 secret 的字段。

如果为了锁屏状态下显示列表或搜索而缓存 title/domain，必须是用户可选的“快速锁屏索引”模式，默认关闭，并在安全设置中明确说明泄露范围。

## 5. Search Index Policy

搜索索引不能成为 vault 的旁路明文副本。

MVP 策略：

- 默认只在解锁后构建内存搜索索引。
- 磁盘索引要么整体加密，要么只保存不可逆 token。
- API key 不做明文全文索引。
- API key 只允许搜索用户自定义 alias、后四位、HMAC fingerprint。
- HMAC key 必须来自 vault 内 index key，不能写在磁盘明文。
- 锁定 vault 时清空内存索引中的敏感字段。

可选增强：

- 加密 SQLite index。
- record-level encrypted index shards。
- 使用 blind index 支持 domain/title 搜索，但需记录泄露模型。

## 6. Sync Policy

iCloud/WebDAV/本地文件夹同步只同步密文对象。

要求：

- 远端服务永远不需要 vault key。
- 远端对象不包含 provider title/domain/endpoint/base URL/console URL/auth scheme/interface type/API key。
- 同步冲突对象仍是密文。
- 合并需要明文时必须等待本地解锁。
- WebDAV ETag/LOCK 只用于并发控制，不作为安全边界。
- 远端篡改必须通过 AEAD/AAD 检测。
- 无法验证的对象进入 quarantine，不覆盖本地数据。

## 7. Backup And Export

默认导出必须是加密备份：

- `.aipass-backup` 本质是 encrypted vault snapshot。
- 包含 KDF params、encrypted vault key、encrypted objects。
- 不包含明文 index。

明文导出：

- 默认隐藏在 advanced。
- 必须二次确认。
- 必须显示不可逆风险。
- 必须要求重新输入 master password。
- 导出文件名带 `UNENCRYPTED`。
- 导出完成后不自动打开文件夹预览，避免误同步。

## 8. CLI Config Writes

默认不把 API key 写入第三方工具配置文件。

优先级：

1. helper mode: 第三方工具运行时调用 `aipass get` 或 tool-specific helper。
2. wrapper mode: `aipass exec <entry> -- tool ...` 注入临时 env。
3. env file mode: 仅写入引用或模板，不写明文。
4. plaintext mode: 用户显式选择，二次确认，并记录审计。

配置 backup 不得包含明文 API key，除非用户原文件本来已有明文；这种情况下 backup 必须被 AIPass 加密保存。

## 9. Runtime Secret Handling

- 解锁后 Vault Key 只保存在 Rust core 内存。
- 前端 WebView 默认拿不到 secret 明文。
- Copy/Fill 由 Rust core、native host 或 OS clipboard bridge 执行。
- Reveal 使用短期 token 和超时清理。
- Clipboard 默认 30-60 秒清理，用户可调。
- 日志、错误、panic hook 必须 redaction。
- Extension service worker 和 content script 不持久化 secret 明文。
- Native Messaging message 不写入日志；debug mode 也必须 redaction。

## 10. Acceptance Tests

每次触碰 vault/crypto/sync/extension/config writer，都必须跑以下测试：

- `fake_key_leak_scan`: 使用固定 fake key 创建 vault 后，扫描 vault、index、logs、backup、extension storage、config backups，不能匹配 fake key。
- `stolen_vault_test`: 不提供 master password 时，离线解析 vault 目录不能得到 title/domain/endpoint/auth scheme/interface type/API key。
- `tamper_test`: 修改 ciphertext、AAD、lamport、object type 后必须解密失败或 quarantine。
- `sync_server_visibility_test`: WebDAV mock server 保存的 payload 不含 fake key、domain、endpoint、auth scheme 或 interface type。
- `locked_state_test`: vault 锁定后 UI/CLI/extension secret API 全部拒绝。
- `plaintext_export_guard_test`: 明文导出必须二次确认并要求 master password。
- `epoch_ratchet_test`: 当前 Vault Epoch Key 不能解密已销毁旧 epoch 的 expired object。
- `ttl_erasure_test`: 删除 TTL key 或 wrapped DEK 后，对应过期对象不可解密，但 active record 不受影响。
- `compromise_recovery_test`: 模拟旧 epoch key 泄露后，advance epoch 并 re-encrypt/rewrap 的新对象不能被旧 key 解密。

## 11. Security Settings

设置页必须提供：

- Auto-lock timeout。
- Clipboard clear timeout。
- Biometric unlock 开关。
- Fast locked search index 开关，默认关闭。
- Plaintext export 开关，默认关闭。
- Plaintext CLI config write 开关，默认关闭。
- Device list 与 revoke。
- Recovery key/backup setup。

## 12. Non-negotiable Release Gate

任何 release 如果无法满足以下条件，不得发布：

- 复制整个 vault/sync/backup 目录不能恢复 API secrets。
- fake key leak scan 为 0。
- 默认 CLI 配置模式不写明文 key。
- Chrome extension storage 不含明文 key。
- 日志和 crash report 不含明文 key。
- 同步服务端只看到密文对象。
- Epoch ratchet、TTL cryptographic erasure 和 compromise recovery 测试通过。

## 13. References

- Signal Double Ratchet specification: https://signal.org/docs/specifications/doubleratchet/
- IETF MLS RFC 9420: https://www.rfc-editor.org/rfc/rfc9420.html
- NIST SP 800-57 Part 1 Rev. 5: https://csrc.nist.gov/pubs/sp/800/57/pt1/r5/final
- libsodium secretstream rekey API: https://libsodium.gitbook.io/doc/secret-key_cryptography/secretstream
