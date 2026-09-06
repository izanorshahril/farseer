import { useEffect, useState } from "react";

export type SensitiveKind = "account" | "path" | "session" | "diagnostic";

// Presentation surfaces are screenshot-safe on first render. This is a
// presentation policy only; the source value stays in the caller and record.
let enabled = true;
const listeners = new Set<(value: boolean) => void>();

export function setPrivacy(value: boolean): void {
  enabled = value;
  for (const listener of [...listeners]) listener(value);
}

export function togglePrivacy(): void {
  setPrivacy(!enabled);
}

export function privacyEnabled(): boolean {
  return enabled;
}

export function onPrivacy(listener: (value: boolean) => void): () => void {
  listeners.add(listener);
  listener(enabled);
  return () => listeners.delete(listener);
}

export function usePrivacy(): boolean {
  const [value, setValue] = useState(enabled);
  useEffect(() => onPrivacy(setValue), []);
  return value;
}

export function mask(value: string, kind: SensitiveKind, active = enabled): string {
  if (!active || !value) return value;
  return kind === "path" ? "path hidden" : kind === "account" ? "account hidden" : "hidden";
}
