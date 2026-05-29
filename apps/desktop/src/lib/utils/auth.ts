import type { PasswordStrength, PasswordStrengthLevel } from "../types";

type Translate = (key: string, params?: Record<string, string | number>) => string;

const fallbackTranslate: Translate = (key, params = {}) => {
  const fallback: Record<string, string> = {
    "password.enter": "Enter a password",
    "password.tooShort": "Too short",
    "password.weak": "Weak",
    "password.fair": "Fair",
    "password.good": "Good",
    "password.strong": "Strong",
    "password.moreChars": "{count} more chars",
    "password.mixCase": "mix case",
    "password.addDigit": "add a digit",
    "password.addSymbol": "add a symbol"
  };
  return (fallback[key] ?? key).replace(/\{(\w+)\}/g, (_, name) => String(params[name] ?? ""));
};

export function passwordStrength(value: string, translate: Translate = fallbackTranslate): PasswordStrength {
  if (!value) {
    return { label: translate("password.enter"), className: "strength-empty", level: "empty", score: 0 };
  }

  const length = value.length;
  const hasLower = /[a-z]/.test(value);
  const hasUpper = /[A-Z]/.test(value);
  const hasDigit = /\d/.test(value);
  const hasSymbol = /[^A-Za-z0-9]/.test(value);
  const variety = [hasLower, hasUpper, hasDigit, hasSymbol].filter(Boolean).length;

  let score = 0;
  if (length >= 8) score += 1;
  if (length >= 12) score += 1;
  if (length >= 16) score += 1;
  if (length >= 20) score += 1;
  if (variety >= 2) score += 1;
  if (variety >= 3) score += 1;
  if (variety >= 4) score += 1;

  let level: PasswordStrengthLevel;
  let label: string;
  if (length < 8) {
    level = "weak";
    label = translate("password.tooShort");
  } else if (score <= 2) {
    level = "weak";
    label = translate("password.weak");
  } else if (score <= 4) {
    level = "fair";
    label = translate("password.fair");
  } else if (score <= 5) {
    level = "good";
    label = translate("password.good");
  } else {
    level = "strong";
    label = translate("password.strong");
  }

  const hint = buildHint(length, hasLower, hasUpper, hasDigit, hasSymbol, translate);

  return {
    label,
    className: `strength-${level}`,
    level,
    score,
    hint
  };
}

function buildHint(
  length: number,
  hasLower: boolean,
  hasUpper: boolean,
  hasDigit: boolean,
  hasSymbol: boolean,
  translate: Translate
): string | undefined {
  const tips: string[] = [];
  if (length < 12) tips.push(translate("password.moreChars", { count: Math.max(0, 12 - length) }));
  if (!hasUpper || !hasLower) tips.push(translate("password.mixCase"));
  if (!hasDigit) tips.push(translate("password.addDigit"));
  if (!hasSymbol) tips.push(translate("password.addSymbol"));
  if (tips.length === 0) return undefined;
  return tips.slice(0, 2).join(" · ");
}
