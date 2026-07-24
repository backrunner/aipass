const compactFormatter = new Intl.NumberFormat(undefined, {
  notation: "compact",
  maximumFractionDigits: 1
});

/** 大数字紧凑格式化：1234 → "1.2K"，避免长数字撑破布局。 */
export function formatCompact(value: number): string {
  if (!Number.isFinite(value)) return "0";
  if (Math.abs(value) < 1000) return Math.round(value).toLocaleString();
  return compactFormatter.format(value);
}

/** micros（1e-6 USD）→ 美元字符串；小值保留有效数字，大值紧凑。 */
export function formatCostMicros(micros: number): string {
  if (!Number.isFinite(micros) || micros <= 0) return "$0";
  const usd = micros / 1e6;
  if (usd >= 1000) return `$${compactFormatter.format(usd)}`;
  return `$${usd.toLocaleString(undefined, { maximumSignificantDigits: 4 })}`;
}
