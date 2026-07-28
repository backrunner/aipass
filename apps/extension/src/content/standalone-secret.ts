/**
 * Generated keys are normally rendered as the value of one control or text
 * block. Reject prose, commands, JSON, and other larger strings that merely
 * contain a key-shaped fragment.
 */
export function isStandaloneSecretBlock(
  value: string,
  secret: string,
): boolean {
  const normalized = value
    .trim()
    .replace(
      /^(?:api\s*(?:key|token)|secret(?:\s*key)?|access\s*token|key|token|密钥|令牌)(?:\s*[:：=]\s*|\s+)/i,
      "",
    )
    .replace(/^bearer\s+/i, "")
    .replace(/\s+(?:copy(?:\s+(?:key|token))?|复制(?:密钥|令牌)?)$/i, "")
    .replace(/^[\x60"'([{<]+/, "")
    .replace(/[\x60"')\]},.;:>]+$/, "")
    .trim();
  return normalized === secret;
}
