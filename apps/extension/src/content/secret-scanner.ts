export type SecretCandidate = {
  secret: string;
  label?: string;
  gateway?: {
    group?: string;
    rate?: string;
  };
};

export const SELF_HOSTED_TOKEN_PATH_PATTERN =
  /\/(console\/token|app\/tokens|token|tokens|key|keys|api-?keys|virtual-keys|api-manager|downstream-keys|settings|user)(\/|$)/i;

const SECRET_PATTERNS = [
  /sk-[A-Za-z0-9_-]{12,}/,
  /sk-ant-[A-Za-z0-9_-]{12,}/,
  /r8_[A-Za-z0-9_-]{20,}/,
  /AIza[0-9A-Za-z_-]{20,}/,
  /([A-Za-z0-9_-]{24,}\.[A-Za-z0-9_-]{12,}\.[A-Za-z0-9_-]{12,})/
];
const CONTEXTUAL_SECRET_PATTERN = /[A-Za-z0-9][A-Za-z0-9._-]{15,}/;

export function findSecretCandidates(doc: Document, options: { tokenManagementPage?: boolean } = {}): SecretCandidate[] {
  const candidates: SecretCandidate[] = [];
  const inputs = Array.from(doc.querySelectorAll<HTMLInputElement | HTMLTextAreaElement>("input, textarea"));
  for (const input of inputs) {
    const label = [
      input.name,
      input.id,
      input.placeholder,
      input.getAttribute("aria-label") ?? "",
      input.getAttribute("title") ?? "",
      input.closest("label, section, article, form, div, body")?.textContent?.slice(0, 400) ?? ""
    ]
      .join(" ")
      .toLowerCase();
    const value = input.value.trim();
    if (!value) continue;
    if (hasKeyContext(label)) {
      for (const secret of extractSecrets(value, true)) {
        candidates.push({ secret, ...metadataFromElement(input, secret) });
      }
    }
  }
  const explicitKeyElements = Array.from(
    doc.querySelectorAll<HTMLElement>(
      "code, pre, output, [data-api-key], [data-token], [role='textbox'], [aria-label*='key' i], [aria-label*='token' i], [title*='key' i], [title*='token' i]"
    )
  );
  for (const element of explicitKeyElements.slice(0, 80)) {
    const context = [
      element.getAttribute("aria-label") ?? "",
      element.getAttribute("title") ?? "",
      element.getAttribute("data-api-key") ?? "",
      element.getAttribute("data-token") ?? "",
      element.closest("section, article, form, div, body")?.textContent?.slice(0, 400) ?? ""
    ]
      .join(" ")
      .toLowerCase();
    if (!hasKeyContext(context)) continue;
    const value = (element.textContent ?? "").trim();
    for (const secret of extractSecrets(value, true)) {
      candidates.push({ secret, ...metadataFromElement(element, secret) });
    }
  }
  if (options.tokenManagementPage) {
    candidates.push(...findTokenManagementCandidates(doc));
  }
  return uniqueCandidates(candidates);
}

export function extractSecret(value: string, allowContextual: boolean): string | undefined {
  return extractSecrets(value, allowContextual)[0];
}

export function hasKeyContext(context: string): boolean {
  return /(api|key|token|secret|credential|密钥|令牌)/i.test(context);
}

function extractSecrets(value: string, allowContextual: boolean): string[] {
  const matches: string[] = [];
  for (const pattern of SECRET_PATTERNS) {
    const globalPattern = new RegExp(pattern.source, pattern.flags.includes("g") ? pattern.flags : `${pattern.flags}g`);
    for (const match of value.matchAll(globalPattern)) {
      const candidate = match[0]?.replace(/[),.;]+$/, "");
      if (candidate) matches.push(candidate);
    }
  }
  if (!allowContextual) return Array.from(new Set(matches));
  const contextual = new RegExp(CONTEXTUAL_SECRET_PATTERN.source, "g");
  for (const match of value.matchAll(contextual)) {
    const candidate = match[0].replace(/[),.;]+$/, "");
    if (/sk-/i.test(candidate) && !candidate.toLowerCase().startsWith("sk-")) continue;
    if (matches.some((secret) => secret.includes(candidate))) continue;
    if (isLikelySecret(candidate)) matches.push(candidate);
  }
  return Array.from(new Set(matches));
}

function isLikelySecret(candidate: string): boolean {
  if (/^https?:/i.test(candidate)) return false;
  if (candidate.includes("@")) return false;
  if (/^\d+$/.test(candidate)) return false;
  if (/^[A-F0-9-]{36}$/i.test(candidate)) return false;
  if (!/[A-Za-z]/.test(candidate) || !/\d/.test(candidate)) return false;
  return true;
}

function findTokenManagementCandidates(doc: Document): SecretCandidate[] {
  const rows = Array.from(
    doc.querySelectorAll<HTMLElement>(
      "tr, [role='row'], article, li, section, .ant-table-row, .el-table__row, .semi-table-row, .v-data-table__tr"
    )
  );
  const candidates: SecretCandidate[] = [];
  for (const row of rows.slice(0, 240)) {
    const text = normalizedText(row);
    if (text.length < 16 || text.length > 2000) continue;
    if (!hasKeyContext(text) && !/sk-|AIza|r8_|倍率|分组|group|rate/i.test(text)) continue;
    for (const secret of extractSecrets(text, true)) {
      candidates.push({ secret, ...metadataFromElement(row, secret) });
    }
  }
  return candidates;
}

function metadataFromElement(element: Element, secret: string): Omit<SecretCandidate, "secret"> {
  const row = element.closest("tr, [role='row'], article, li, section, .ant-table-row, .el-table__row, .semi-table-row, .v-data-table__tr");
  const contextElement = row ?? element.closest("label, section, article, form, div, body") ?? element;
  const context = normalizedText(contextElement).replace(secret, " ");
  const tableMetadata = row ? metadataFromTableRow(row, secret) : {};
  const group = tableMetadata.gateway?.group ?? extractGatewayGroup(context);
  const rate = tableMetadata.gateway?.rate ?? extractGatewayRate(context);
  const label = tableMetadata.label ?? extractCandidateLabel(context, secret);
  return {
    label,
    gateway: group || rate ? { group, rate } : undefined
  };
}

function metadataFromTableRow(row: Element, secret: string): Omit<SecretCandidate, "secret"> {
  const cells = Array.from(row.querySelectorAll<HTMLElement>("td, th, [role='cell'], [role='gridcell'], [role='columnheader']"));
  if (!cells.length) return {};
  const headers = tableHeadersForRow(row);
  let label: string | undefined;
  let group: string | undefined;
  let rate: string | undefined;
  cells.forEach((cell, index) => {
    const header = (headers[index] ?? "").toLowerCase();
    const value = cleanedCellText(cell, secret);
    if (!value) return;
    if (!label && /(name|label|名称|备注|说明)/i.test(header)) label = value;
    if (!group && /(group|分组|模型组|渠道组)/i.test(header)) group = value;
    if (!rate && /(rate|ratio|倍率|倍数)/i.test(header)) rate = value;
  });
  return {
    label,
    gateway: group || rate ? { group, rate } : undefined
  };
}

function tableHeadersForRow(row: Element): string[] {
  const table = row.closest("table");
  if (!table) return [];
  const headerRow = table.querySelector("thead tr") ?? table.querySelector("tr");
  if (!headerRow || headerRow === row) return [];
  return Array.from(headerRow.querySelectorAll<HTMLElement>("th, td, [role='columnheader']"))
    .map((cell) => normalizedText(cell))
    .filter(Boolean);
}

function cleanedCellText(cell: Element, secret: string): string {
  return normalizedText(cell)
    .replace(secret, " ")
    .replace(/copy|复制|查看|删除|编辑|启用|禁用/gi, " ")
    .trim();
}

function normalizedText(element: Element): string {
  return (element.textContent ?? "").replace(/\s+/g, " ").trim();
}

function extractGatewayGroup(text: string): string | undefined {
  return extractLabeledValue(text, /(?:分组|模型组|渠道组|group)\s*[:：]?\s*([^\s,，;；|]+)/i);
}

function extractGatewayRate(text: string): string | undefined {
  return extractLabeledValue(
    text,
    /(?:倍率|倍数|rate|ratio|multiplier)\s*[:：]?\s*(x?\d+(?:\.\d+)?x?|\d+(?:\.\d+)?\s*倍)/i
  );
}

function extractLabeledValue(text: string, pattern: RegExp): string | undefined {
  const match = text.match(pattern);
  return sanitizeMetadataValue(match?.[1]);
}

function sanitizeMetadataValue(value: string | undefined): string | undefined {
  const cleaned = value?.trim().replace(/[，,;；。]+$/, "");
  if (!cleaned || hasKeyContext(cleaned) || cleaned.length > 80) return undefined;
  return cleaned;
}

function extractCandidateLabel(text: string, secret: string): string | undefined {
  const cleaned = text
    .replace(secret, " ")
    .replace(/api\s*key|token|secret|密钥|令牌|复制|copy/gi, " ")
    .replace(/\s+/g, " ")
    .trim();
  if (!cleaned || cleaned.length > 64 || /^[-_:|,\s]+$/.test(cleaned)) return undefined;
  return cleaned;
}

function uniqueCandidates(candidates: SecretCandidate[]): SecretCandidate[] {
  const bySecret = new Map<string, SecretCandidate>();
  for (const candidate of candidates) {
    const existing = bySecret.get(candidate.secret);
    if (!existing) {
      bySecret.set(candidate.secret, candidate);
      continue;
    }
    bySecret.set(candidate.secret, {
      secret: candidate.secret,
      label: existing.label ?? candidate.label,
      gateway: {
        group: existing.gateway?.group ?? candidate.gateway?.group,
        rate: existing.gateway?.rate ?? candidate.gateway?.rate
      }
    });
  }
  return Array.from(bySecret.values()).map((candidate) => ({
    ...candidate,
    gateway:
      candidate.gateway?.group || candidate.gateway?.rate
        ? candidate.gateway
        : undefined
  }));
}
