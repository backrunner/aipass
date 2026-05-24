import type { PasswordStrength } from "../types";

export function passwordStrength(value: string): PasswordStrength {
  const score =
    (value.length >= 16 ? 2 : value.length >= 12 ? 1 : 0) +
    (/[A-Z]/.test(value) ? 1 : 0) +
    (/[a-z]/.test(value) ? 1 : 0) +
    (/\d/.test(value) ? 1 : 0) +
    (/[^A-Za-z0-9]/.test(value) ? 1 : 0);
  if (score >= 5) return { label: "Strong", className: "strength-strong" };
  if (score >= 3) return { label: "Good", className: "strength-good" };
  return { label: "Weak", className: "strength-weak" };
}
