import { isStandaloneSecretBlock } from "./standalone-secret";

export type SecretCandidate = {
  secret: string;
  label?: string;
  gateway?: {
    group?: string;
    rate?: string;
  };
};

const TOKEN_ROW_SELECTOR =
  "tr, [role='row'], [data-row-key], .ant-table-row, .arco-table-tr, .arco-table-row, .el-table__row, .semi-table-row, .v-data-table__tr, .ant-list-item, .semi-list-item";

/** Mask runs pages use to elide secrets: "…", "...", "***", "•••". */
const MASK_RUN_SOURCE = "(?:…|\\.{3,}|\\*{2,}|•{2,})";
const MASKED_TOKEN_GLOBAL = new RegExp(
  `[A-Za-z0-9][A-Za-z0-9_-]*${MASK_RUN_SOURCE}[A-Za-z0-9_-]*[A-Za-z0-9]`,
  "g"
);
const MASKED_TOKEN_EXACT = new RegExp(
  `^[A-Za-z0-9][A-Za-z0-9_-]*${MASK_RUN_SOURCE}[A-Za-z0-9_-]*[A-Za-z0-9]$`
);
const MASK_RUN_SPLIT = /…|\.{3,}|\*{2,}|•{2,}/;
const MIN_MASKED_TOKEN_LENGTH = 8;
const MAX_GROUP_LENGTH = 48;
const MAX_RATE_LENGTH = 24;
const MAX_LABEL_LENGTH = 64;

const GROUP_HEADER_PATTERN = /(group|分组|模型组|渠道组)/i;
const RATE_HEADER_PATTERN = /(rate|ratio|multiplier|倍率|倍数)/i;
/** `1.5`, `x1.5`, `1.5x`, `1.5 倍` — the shapes relay consoles print. */
const RATE_VALUE_SOURCE = "x\\s*\\d+(?:\\.\\d+)?|\\d+(?:\\.\\d+)?\\s*x|\\d+(?:\\.\\d+)?\\s*倍|\\d+(?:\\.\\d+)?";
/** Same, minus the bare decimal: unambiguous even without a 倍率 label. */
const MARKED_RATE_VALUE_SOURCE = "x\\s*\\d+(?:\\.\\d+)?|\\d+(?:\\.\\d+)?\\s*x|\\d+(?:\\.\\d+)?\\s*倍";
const LABELED_RATE_PATTERN = new RegExp(
  `(?:倍率|倍数|rate|ratio|multiplier)\\s*[:：=]?\\s*(${RATE_VALUE_SOURCE})`,
  "i"
);
const MARKED_RATE_PATTERN = new RegExp(`^(${MARKED_RATE_VALUE_SOURCE})$`, "i");
const BARE_RATE_PATTERN = /^(\d+(?:\.\d+)?)$/;
/** Option lists that a relay renders its group picker with. */
const GROUP_OPTION_SELECTOR = [
  "option",
  "[role='option']",
  ".ant-select-item-option",
  ".ant-select-item",
  ".semi-select-option",
  ".arco-select-option",
  ".el-select-dropdown__item",
  ".el-option"
].join(", ");
const GROUP_OPTION_OWNER_SELECTOR =
  "select, [role='listbox'], [role='combobox'], .ant-select, .semi-select, .arco-select, .el-select, label, .ant-form-item, .semi-form-field, .el-form-item";
const GROUP_OPTION_SCAN_LIMIT = 120;
const GROUP_RATE_ROW_SCAN_LIMIT = 200;
/**
 * Longest context we still treat as belonging to one token. Anything larger is
 * page-wide text, where a 倍率 almost certainly belongs to a different group.
 */
const SCOPED_CONTEXT_MAX_LENGTH = 600;
const GROUP_RATE_CACHE_TTL_MS = 500;

type GroupRates = Map<string, string>;

const groupRateCache = new WeakMap<Document, { rates: GroupRates; at: number }>();

type SecretScanOptions = {
  providerId?: string;
  tokenManagementPage?: boolean;
  allowOpenAiStyle?: boolean;
  allowCustomKey?: boolean;
  allowEmbeddedSecrets?: boolean;
};

type ExtractSecretOptions = {
  providerId?: string;
  allowOpenAiStyle?: boolean;
  allowCustomKey?: boolean;
  allowEmbeddedSecrets?: boolean;
};

export const SELF_HOSTED_TOKEN_PATH_PATTERN =
  /\/(console\/token|app\/tokens|token|tokens|key|keys|api[-_]?keys?|virtual-keys|api-manager|downstream-keys|settings|user)(\/|$)/i;

const PROVIDER_STRICT_PATTERNS: Record<string, RegExp[]> = {
  anthropic: [/sk-ant-[A-Za-z0-9_-]{12,}/],
  openrouter: [/sk-or-v1-[A-Za-z0-9_-]{16,}/, /sk-or-[A-Za-z0-9_-]{16,}/],
  replicate: [/r8_[A-Za-z0-9_-]{37}(?![A-Za-z0-9_-])/],
  gemini: [/AIza[0-9A-Za-z_-]{35}(?![0-9A-Za-z_-])/],
  groq: [/gsk_[A-Za-z0-9_-]{20,}/],
  fireworks: [/fw_[A-Za-z0-9_-]{20,}/],
  xai: [/xai-[A-Za-z0-9_-]{16,}/],
  perplexity: [/pplx-[A-Za-z0-9_-]{12,}/],
  cerebras: [/csk[-_][A-Za-z0-9_-]{12,}/],
  nvidia: [/nvapi-[A-Za-z0-9_-]{16,}/],
  huggingface: [/hf_[A-Za-z0-9]{20,}/]
};
const PROVIDER_OPENAI_STYLE_IDS = new Set([
  "openai",
  "deepseek",
  "moonshot",
  "qwen",
  "zhipu",
  "volcengine",
  "together",
  "siliconflow",
  "mistral",
  "novita",
  "new_api",
  "one_api",
  "litellm",
  "sub2api",
  "veloera",
  "omniroute",
  "metapi",
  "custom_openai_compatible"
]);
const OPENAI_STYLE_SECRET_PATTERN = /sk-[A-Za-z0-9_-]{12,}/;
const CUSTOM_KEY_SECRET_PATTERN = /[A-Za-z][A-Za-z0-9_-]{1,64}_key_[A-Za-z0-9_-]{12,}/;
const GLOBAL_SECRET_PATTERNS = [
  ...Object.values(PROVIDER_STRICT_PATTERNS).flat(),
  OPENAI_STYLE_SECRET_PATTERN
];
const SECRET_VALUE_ATTRIBUTES = [
  "data-api-key",
  "data-token",
  "data-key",
  "data-secret",
  "data-clipboard-text",
  "data-clipboard",
  "data-copy-text",
  "data-copy"
];
const ROW_SECRET_BLOCK_SELECTOR = [
  "code",
  "pre",
  "output",
  "samp",
  "kbd",
  "td",
  "th",
  "[role='cell']",
  "[role='gridcell']",
  "[role='textbox']",
  "span",
  "p"
].join(", ");
const INPUT_SCAN_LIMIT = 160;
const EXPLICIT_KEY_ELEMENT_SCAN_LIMIT = 80;
const TOKEN_ROW_SCAN_LIMIT = 240;
const TABLE_CELL_SCAN_LIMIT = 24;
const ROW_SECRET_BLOCK_SCAN_LIMIT = 48;

export function findSecretCandidates(doc: Document, options: SecretScanOptions = {}): SecretCandidate[] {
  const candidates: SecretCandidate[] = [];
  const inputs = limitedElements<HTMLInputElement | HTMLTextAreaElement>(doc, "input, textarea", INPUT_SCAN_LIMIT);
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
      for (const secret of extractEligibleSecrets(value, options)) {
        candidates.push({ secret, ...metadataFromElement(input, secret) });
      }
    }
  }
  const explicitKeyElements = limitedElements<HTMLElement>(
    doc,
    "code, pre, output, [data-api-key], [data-token], [data-key], [data-secret], [data-clipboard-text], [data-clipboard], [data-copy-text], [data-copy], [role='textbox'], [aria-label*='key' i], [aria-label*='token' i], [title*='key' i], [title*='token' i]",
    EXPLICIT_KEY_ELEMENT_SCAN_LIMIT
  );
  for (const element of explicitKeyElements) {
    const attributeValues = secretAttributeValues(element);
    const context = [
      element.getAttribute("aria-label") ?? "",
      element.getAttribute("title") ?? "",
      ...SECRET_VALUE_ATTRIBUTES.filter((attribute) => element.hasAttribute(attribute)),
      element.closest("section, article, form, div, body")?.textContent?.slice(0, 400) ?? ""
    ]
      .join(" ")
      .toLowerCase();
    if (!hasKeyContext(context) && !attributeValues.length) continue;
    const sources = [(element.textContent ?? "").trim(), ...attributeValues];
    for (const source of sources) {
      for (const secret of extractEligibleSecrets(source, options)) {
        candidates.push({ secret, ...metadataFromElement(element, secret) });
      }
    }
  }
  if (options.tokenManagementPage) {
    candidates.push(
      ...findTokenManagementCandidates(
        doc,
        options.providerId,
        options.allowEmbeddedSecrets,
      ),
    );
  }
  return uniqueCandidates(candidates);
}

export function extractSecret(
  value: string,
  allowContextualOrOptions: boolean | ExtractSecretOptions
): string | undefined {
  const options =
    typeof allowContextualOrOptions === "boolean"
      ? { allowOpenAiStyle: true, allowCustomKey: allowContextualOrOptions }
      : allowContextualOrOptions;
  return extractEligibleSecrets(value, options)[0];
}

export function hasKeyContext(context: string): boolean {
  return /(api|key|token|secret|credential|密钥|令牌)/i.test(context);
}

function secretAttributeValues(element: Element): string[] {
  return SECRET_VALUE_ATTRIBUTES.map((attribute) => element.getAttribute(attribute)?.trim() ?? "").filter(Boolean);
}

function extractSecrets(value: string, options: ExtractSecretOptions): string[] {
  const matches: string[] = [];
  for (const pattern of secretPatternsForProvider(options)) {
    const globalPattern = new RegExp(pattern.source, pattern.flags.includes("g") ? pattern.flags : `${pattern.flags}g`);
    for (const match of value.matchAll(globalPattern)) {
      const candidate = normalizeSecretMatch(match[1] ?? match[0]);
      if (candidate) matches.push(candidate);
    }
  }
  return Array.from(new Set(matches));
}

function extractStandaloneSecrets(value: string, options: ExtractSecretOptions): string[] {
  return extractSecrets(value, options).filter((secret) => isStandaloneSecretBlock(value, secret));
}

function extractEligibleSecrets(value: string, options: ExtractSecretOptions): string[] {
  return options.allowEmbeddedSecrets
    ? extractSecrets(value, options)
    : extractStandaloneSecrets(value, options);
}

function secretPatternsForProvider(options: ExtractSecretOptions): RegExp[] {
  const providerId = options.providerId;
  if (providerId && PROVIDER_STRICT_PATTERNS[providerId]) return PROVIDER_STRICT_PATTERNS[providerId];
  if (providerId && PROVIDER_OPENAI_STYLE_IDS.has(providerId)) {
    return options.allowCustomKey ? [OPENAI_STYLE_SECRET_PATTERN, CUSTOM_KEY_SECRET_PATTERN] : [OPENAI_STYLE_SECRET_PATTERN];
  }
  if (providerId) return options.allowCustomKey ? [CUSTOM_KEY_SECRET_PATTERN] : [];
  const patterns = options.allowOpenAiStyle ? [...GLOBAL_SECRET_PATTERNS] : Object.values(PROVIDER_STRICT_PATTERNS).flat();
  if (options.allowCustomKey) patterns.push(CUSTOM_KEY_SECRET_PATTERN);
  return patterns;
}

function normalizeSecretMatch(value: string | undefined): string {
  return value?.replace(/^[("'[{<]+/, "").replace(/[)"'\]},.;:>]+$/, "") ?? "";
}

function findTokenManagementCandidates(
  doc: Document,
  providerId?: string,
  allowEmbeddedSecrets = false,
): SecretCandidate[] {
  const rows = limitedElements<HTMLElement>(
    doc,
    TOKEN_ROW_SELECTOR,
    TOKEN_ROW_SCAN_LIMIT
  );
  const candidates: SecretCandidate[] = [];
  for (const row of rows) {
    const text = normalizedText(row);
    if (text.length < 16 || text.length > 2000) continue;
    if (!hasKeyContext(text) && !/sk-|AIza|r8_|倍率|分组|group|rate/i.test(text)) continue;
    for (const source of secretSourcesForRow(row)) {
      const extractOptions = {
        providerId,
        allowOpenAiStyle: true,
        allowCustomKey: providerId === "sub2api",
        allowEmbeddedSecrets
      };
      for (const secret of extractEligibleSecrets(source, extractOptions)) {
        candidates.push({ secret, ...metadataFromElement(row, secret) });
      }
    }
  }
  return candidates;
}

function secretSourcesForRow(row: Element): string[] {
  const cells = limitedElements<HTMLElement>(row, "td, th, [role='cell'], [role='gridcell']", TABLE_CELL_SCAN_LIMIT)
    .map((cell) => normalizedText(cell))
    .filter(Boolean);
  const blocks = limitedElements<HTMLElement>(row, ROW_SECRET_BLOCK_SELECTOR, ROW_SECRET_BLOCK_SCAN_LIMIT)
    .map((element) => normalizedText(element))
    .filter(Boolean);
  const attributed = [row, ...limitedElements<HTMLElement>(
    row,
    "[data-api-key], [data-token], [data-key], [data-secret], [data-clipboard-text], [data-clipboard], [data-copy-text], [data-copy]",
    ROW_SECRET_BLOCK_SCAN_LIMIT
  )].flatMap(secretAttributeValues);
  return Array.from(new Set([...cells, ...blocks, ...attributed]));
}

function metadataFromElement(element: Element, secret: string): Omit<SecretCandidate, "secret"> {
  const row = element.closest(TOKEN_ROW_SELECTOR);
  const contextElement = row ?? element.closest("label, section, article, form, div, body") ?? element;
  const context = normalizedText(contextElement).replace(secret, " ");
  const tableMetadata = row ? metadataFromTableRow(row, secret) : {};
  const group = tableMetadata.gateway?.group ?? extractGatewayGroup(context);
  return {
    label: tableMetadata.label ?? extractCandidateLabel(context, secret),
    gateway: gatewayFor(element.ownerDocument, group, tableMetadata.gateway?.rate, context, Boolean(row))
  };
}

/**
 * Pairs a token's group with its multiplier, most specific source first:
 * the token's own row, then text scoped to that token, then the group→rate
 * table the console publishes elsewhere on the page.
 *
 * Relay consoles (new-api, one-api, veloera) usually list 分组 per token but
 * print 倍率 once per group in the group picker or a pricing table, so a
 * page-wide 倍率 scan would hand a token the wrong group's multiplier.
 */
function gatewayFor(
  doc: Document | null,
  group: string | undefined,
  rowRate: string | undefined,
  context: string,
  rowScoped: boolean
): SecretCandidate["gateway"] {
  const scoped = rowScoped || context.length <= SCOPED_CONTEXT_MAX_LENGTH;
  const rate =
    rowRate ??
    (scoped ? extractGatewayRate(context) : undefined) ??
    publishedRateForGroup(doc, group);
  return group || rate ? { group, rate } : undefined;
}

function publishedRateForGroup(doc: Document | null, group: string | undefined): string | undefined {
  if (!doc || !group) return undefined;
  return groupRatesFor(doc).get(groupKey(group));
}

function groupKey(group: string): string {
  return group.replace(/\s+/g, "").toLowerCase();
}

function groupRatesFor(doc: Document): GroupRates {
  const cached = groupRateCache.get(doc);
  const now = Date.now();
  if (cached && now - cached.at < GROUP_RATE_CACHE_TTL_MS) return cached.rates;
  const rates = collectGroupRates(doc);
  groupRateCache.set(doc, { rates, at: now });
  return rates;
}

/**
 * Group → multiplier as published by the page: the group picker's options
 * (`vip (倍率: 1.5)`) and any table pairing a 分组 column with a 倍率 column.
 * First value wins, so the topmost listing of a group defines its rate.
 */
export function collectGroupRates(doc: Document): GroupRates {
  const rates: GroupRates = new Map();
  const remember = (group?: string, rate?: string) => {
    if (!group || !rate) return;
    const key = groupKey(group);
    if (!rates.has(key)) rates.set(key, rate);
  };

  for (const option of limitedElements<HTMLElement>(doc, GROUP_OPTION_SELECTOR, GROUP_OPTION_SCAN_LIMIT)) {
    const entry = parseGroupRateOption(normalizedText(option), optionAllowsBareRate(option));
    remember(entry?.group, entry?.rate);
  }

  for (const row of limitedElements<HTMLElement>(doc, "tr, [role='row']", GROUP_RATE_ROW_SCAN_LIMIT)) {
    const paired = groupRatePairFromRow(row);
    remember(paired?.group, paired?.rate);
  }

  return rates;
}

/** A group-picker option such as `vip (倍率: 1.5)`, `default (1x)`, `vip - 1.5倍`. */
function parseGroupRateOption(
  text: string,
  allowBareRate: boolean
): { group?: string; rate?: string } | undefined {
  if (!text || text.length > MAX_GROUP_LENGTH + MAX_RATE_LENGTH + 24) return undefined;

  // `分组：vip 倍率：1.5` — both sides labelled, so no positional guessing.
  const labelledGroup = extractGatewayGroup(text);
  if (labelledGroup) {
    const labelledRate = extractGatewayRate(text);
    if (labelledRate) return { group: labelledGroup, rate: labelledRate };
  }

  const split =
    text.match(/^(.+?)\s*[（(\[]\s*([^（()）\[\]]{1,24})\s*[)）\]]\s*$/) ??
    text.match(/^(.+?)\s*[-–—|,，、:：]\s*([^-–—|,，、]{1,24})\s*$/);
  if (!split) return undefined;
  const group = sanitizeMetadataValue(split[1]);
  const rate = rateFromFragment(split[2] ?? "", allowBareRate);
  if (!group || !rate || GROUP_HEADER_PATTERN.test(group)) return undefined;
  return { group, rate };
}

function rateFromFragment(fragment: string, allowBareRate: boolean): string | undefined {
  const trimmed = fragment.trim();
  if (!trimmed) return undefined;
  const labelled = trimmed.match(LABELED_RATE_PATTERN);
  if (labelled?.[1]) return normalizeRate(labelled[1]);
  if (MARKED_RATE_PATTERN.test(trimmed)) return normalizeRate(trimmed);
  if (allowBareRate && BARE_RATE_PATTERN.test(trimmed)) return normalizeRate(trimmed);
  return undefined;
}

function normalizeRate(value: string): string | undefined {
  const cleaned = value.replace(/\s+/g, "").trim();
  return cleaned && cleaned.length <= MAX_RATE_LENGTH ? cleaned : undefined;
}

/**
 * A bare number in a group option is only a multiplier when the control it
 * belongs to is clearly a group picker; elsewhere `Plan (3)` could be anything.
 */
function optionAllowsBareRate(option: Element): boolean {
  const owner = option.closest(GROUP_OPTION_OWNER_SELECTOR);
  if (!owner) return false;
  const context = [
    owner.getAttribute("name") ?? "",
    owner.getAttribute("id") ?? "",
    owner.getAttribute("aria-label") ?? "",
    owner.getAttribute("placeholder") ?? "",
    owner instanceof HTMLSelectElement ? labelTextForControl(owner) : ""
  ].join(" ");
  return GROUP_HEADER_PATTERN.test(context);
}

function labelTextForControl(control: HTMLSelectElement): string {
  const wrapping = control.closest("label");
  if (wrapping) return normalizedText(wrapping);
  const id = control.getAttribute("id");
  if (!id) return "";
  const doc = control.ownerDocument;
  const labelled = doc?.querySelector(`label[for="${CSS.escape(id)}"]`);
  return labelled ? normalizedText(labelled) : "";
}

/** A table row pairing a 分组 cell with a 倍率 cell, by column header. */
function groupRatePairFromRow(row: Element): { group?: string; rate?: string } | undefined {
  const headers = tableHeadersForRow(row);
  if (!headers.length) return undefined;
  const groupIndex = headers.findIndex((header) => GROUP_HEADER_PATTERN.test(header));
  const rateIndex = headers.findIndex((header) => RATE_HEADER_PATTERN.test(header));
  if (groupIndex < 0 || rateIndex < 0) return undefined;
  const cells = limitedElements<HTMLElement>(
    row,
    "td, th, [role='cell'], [role='gridcell']",
    TABLE_CELL_SCAN_LIMIT
  );
  const group = sanitizeMetadataValue(sanitizeCellValue(normalizedText(cells[groupIndex] ?? row)));
  const rate = rateFromFragment(sanitizeCellValue(normalizedText(cells[rateIndex] ?? row)) ?? "", true);
  if (!group || !rate) return undefined;
  return { group, rate };
}

export function metadataForSecretElement(element: Element, secret: string): Omit<SecretCandidate, "secret"> {
  return metadataFromElement(element, secret);
}

/**
 * Locates the token-table row that displays an elided form of `secret`
 * (e.g. `sk-abc…xyz`) and extracts label/group/rate from that row. Used when
 * the full secret is only known from the clipboard or framework state, so the
 * DOM scanner cannot match it directly.
 */
export function metadataForMaskedSecret(
  doc: Document,
  secret: string
): Omit<SecretCandidate, "secret"> | undefined {
  const rows = limitedElements<HTMLElement>(doc, TOKEN_ROW_SELECTOR, TOKEN_ROW_SCAN_LIMIT);
  for (const row of rows) {
    if (!rowDisplaysMaskedSecret(row, secret)) continue;
    const context = normalizedText(row).replace(secret, " ");
    const tableMetadata = metadataFromTableRow(row, secret);
    const group = tableMetadata.gateway?.group ?? extractGatewayGroup(context);
    return {
      label: tableMetadata.label ?? extractCandidateLabel(context, secret),
      gateway: gatewayFor(doc, group, tableMetadata.gateway?.rate, context, true)
    };
  }
  return undefined;
}

function rowDisplaysMaskedSecret(row: Element, secret: string): boolean {
  const cells = limitedElements<HTMLElement>(row, "td, th, [role='cell'], [role='gridcell']", TABLE_CELL_SCAN_LIMIT);
  const texts = cells.length ? cells.map((cell) => normalizedText(cell)) : [normalizedText(row)];
  return texts.some((text) => textDisplaysMaskedSecret(text, secret));
}

function textDisplaysMaskedSecret(text: string, secret: string): boolean {
  if (text.includes(secret)) return true;
  for (const match of text.matchAll(MASKED_TOKEN_GLOBAL)) {
    if (maskedTokenMatchesSecret(match[0], secret)) return true;
  }
  return false;
}

/**
 * Checks whether an elided page token (e.g. `sk-abc…xyz`, `sk-abc***xyz`)
 * is consistent with the full secret: literal segments around the mask run
 * must anchor the secret (first segment = prefix, last = suffix).
 */
export function maskedTokenMatchesSecret(token: string, secret: string): boolean {
  if (token.length < MIN_MASKED_TOKEN_LENGTH || !MASKED_TOKEN_EXACT.test(token)) return false;
  const segments = token.split(MASK_RUN_SPLIT).filter(Boolean);
  if (segments.length < 2) return false;
  const first = segments[0];
  const last = segments[segments.length - 1];
  if (first.length < 3 || last.length < 3) return false;
  if (!secret.startsWith(first) || !secret.endsWith(last)) return false;
  let position = first.length;
  for (const segment of segments.slice(1, -1)) {
    const index = secret.indexOf(segment, position);
    if (index < 0) return false;
    position = index + segment.length;
  }
  return true;
}

/** True when the whole value is an elided secret (e.g. `sk-abc…xyz`). */
export function looksLikeMaskedSecret(value: string): boolean {
  return value.length >= MIN_MASKED_TOKEN_LENGTH && MASKED_TOKEN_EXACT.test(value);
}

/** True when the value contains an unmasked secret shape. */
export function containsSecretLike(value: string): boolean {
  for (const pattern of [...GLOBAL_SECRET_PATTERNS, CUSTOM_KEY_SECRET_PATTERN]) {
    if (pattern.test(value)) return true;
  }
  return false;
}

function metadataFromTableRow(row: Element, secret: string): Omit<SecretCandidate, "secret"> {
  const cells = limitedElements<HTMLElement>(row, "td, th, [role='cell'], [role='gridcell'], [role='columnheader']", TABLE_CELL_SCAN_LIMIT);
  if (!cells.length) return {};
  const headers = tableHeadersForRow(row);
  let label: string | undefined;
  let group: string | undefined;
  let rate: string | undefined;
  cells.forEach((cell, index) => {
    const header = (headers[index] ?? "").toLowerCase();
    const value = sanitizeCellValue(cleanedCellText(cell, secret));
    if (!value) return;
    if (!label && /(name|label|名称|备注|说明)/i.test(header)) label = truncateMetadata(value, MAX_LABEL_LENGTH);
    if (!group && GROUP_HEADER_PATTERN.test(header)) group = truncateMetadata(value, MAX_GROUP_LENGTH);
    // A 倍率 column holds a multiplier and nothing else — reject anything that
    // is not one, so a placeholder never becomes a billing rate.
    if (!rate && RATE_HEADER_PATTERN.test(header)) rate = rateFromFragment(value, true);
  });
  return {
    label,
    gateway: group || rate ? { group, rate } : undefined
  };
}

/** Rejects cell values that are really the elided key itself (or a raw secret). */
function sanitizeCellValue(value: string): string | undefined {
  if (!value) return undefined;
  if (looksLikeMaskedSecret(value) || containsSecretLike(value)) return undefined;
  return value;
}

function truncateMetadata(value: string, maxLength: number): string | undefined {
  return value.length > maxLength ? undefined : value;
}

function limitedElements<T extends Element>(
  root: Document | Element,
  selector: string,
  limit: number
): T[] {
  const nodes = root.querySelectorAll<T>(selector);
  const elements: T[] = [];
  for (let index = 0; index < nodes.length && elements.length < limit; index += 1) {
    const element = nodes.item(index);
    if (element) elements.push(element);
  }
  return elements;
}

function tableHeadersForRow(row: Element): string[] {
  const table = row.closest("table");
  if (!table) return [];
  const headerRow = table.querySelector("thead tr") ?? table.querySelector("tr");
  if (!headerRow || headerRow === row) return [];
  return limitedElements<HTMLElement>(headerRow, "th, td, [role='columnheader']", TABLE_CELL_SCAN_LIMIT)
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
  const match = text.match(LABELED_RATE_PATTERN);
  return match?.[1] ? normalizeRate(match[1]) : undefined;
}

function extractLabeledValue(text: string, pattern: RegExp): string | undefined {
  const match = text.match(pattern);
  return sanitizeMetadataValue(match?.[1]);
}

function sanitizeMetadataValue(value: string | undefined): string | undefined {
  const cleaned = value?.trim().replace(/[，,;；。]+$/, "");
  if (
    !cleaned ||
    hasKeyContext(cleaned) ||
    cleaned.length > MAX_GROUP_LENGTH ||
    looksLikeMaskedSecret(cleaned) ||
    containsSecretLike(cleaned)
  ) {
    return undefined;
  }
  return cleaned;
}

function extractCandidateLabel(text: string, secret: string): string | undefined {
  const cleaned = text
    .replace(secret, " ")
    .replace(MASKED_TOKEN_GLOBAL, " ")
    .replace(/api\s*key|token|secret|密钥|令牌|复制|copy/gi, " ")
    .replace(/\s+/g, " ")
    .trim();
  if (
    !cleaned ||
    cleaned.length > MAX_LABEL_LENGTH ||
    /^[-_:|,\s]+$/.test(cleaned) ||
    looksLikeMaskedSecret(cleaned) ||
    containsSecretLike(cleaned)
  ) {
    return undefined;
  }
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
