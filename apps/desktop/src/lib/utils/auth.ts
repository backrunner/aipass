import type { PasswordStrength, PasswordStrengthLevel } from "../types";

export function passwordStrength(value: string): PasswordStrength {
  if (!value) {
    return { label: "Enter a password", className: "strength-empty", level: "empty", score: 0 };
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
    label = "Too short";
  } else if (score <= 2) {
    level = "weak";
    label = "Weak";
  } else if (score <= 4) {
    level = "fair";
    label = "Fair";
  } else if (score <= 5) {
    level = "good";
    label = "Good";
  } else {
    level = "strong";
    label = "Strong";
  }

  const hint = buildHint(length, hasLower, hasUpper, hasDigit, hasSymbol);

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
  hasSymbol: boolean
): string | undefined {
  const tips: string[] = [];
  if (length < 12) tips.push(`${Math.max(0, 12 - length)} more chars`);
  if (!hasUpper || !hasLower) tips.push("mix case");
  if (!hasDigit) tips.push("add a digit");
  if (!hasSymbol) tips.push("add a symbol");
  if (tips.length === 0) return undefined;
  return tips.slice(0, 2).join(" · ");
}
