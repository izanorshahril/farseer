import { useEffect, useState, type ReactNode } from "react";

export type SensitiveKind = "account" | "path" | "session" | "diagnostic";

// Presentation surfaces are screenshot-safe on first render. This is a
// presentation policy only; the source value stays in the caller and record.
let enabled = true;
const listeners = new Set<(value: boolean) => void>();
const revealListeners = new Set<(key: string) => void>();
const revealed = new Set<string>();
const revealTimers = new Map<string, ReturnType<typeof setTimeout>>();

if (typeof document !== "undefined") {
  document.addEventListener("visibilitychange", () => {
    if (document.hidden) setPrivacy(true);
  });
}

export function setPrivacy(value: boolean): void {
  enabled = value;
  if (value) clearReveals();
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

function clearReveals(): void {
  for (const timer of revealTimers.values()) clearTimeout(timer);
  revealTimers.clear();
  revealed.clear();
}

export function revealField(key: string, durationMs = 10_000): void {
  if (!key) return;
  const prior = revealTimers.get(key);
  if (prior) clearTimeout(prior);
  revealed.add(key);
  revealTimers.set(key, setTimeout(() => {
    revealed.delete(key);
    revealTimers.delete(key);
    for (const listener of [...revealListeners]) listener(key);
  }, durationMs));
  for (const listener of [...revealListeners]) listener(key);
}

export function isFieldRevealed(key: string): boolean {
  return !enabled || revealed.has(key);
}

export function onReveal(listener: (key: string) => void): () => void {
  revealListeners.add(listener);
  return () => revealListeners.delete(listener);
}

export function usePrivacy(): boolean {
  const [value, setValue] = useState(enabled);
  useEffect(() => onPrivacy(setValue), []);
  return value;
}

export function useRevealed(key: string): boolean {
  const [value, setValue] = useState(() => isFieldRevealed(key));
  useEffect(() => {
    const refresh = (changed: string) => {
      if (changed === key) setValue(isFieldRevealed(key));
    };
    const stopReveal = onReveal(refresh);
    const stopPrivacy = onPrivacy(() => setValue(isFieldRevealed(key)));
    return () => {
      stopReveal();
      stopPrivacy();
    };
  }, [key]);
  return value;
}

export function mask(value: string, kind: SensitiveKind, active = enabled): string {
  if (!active || !value) return value;
  return kind === "path" ? "path hidden" : kind === "account" ? "account hidden" : "hidden";
}

/** Copy/export callers use the same presentation policy as visible text. */
export function presentationValue(value: string, kind: SensitiveKind, active = enabled): string {
  return mask(value, kind, active);
}

export function RevealField({
  value,
  kind,
  fieldKey,
  label = "sensitive field",
}: {
  value: string;
  kind: SensitiveKind;
  fieldKey: string;
  label?: string;
}): ReactNode {
  const visible = useRevealed(fieldKey);
  const display = visible ? value : mask(value, kind);
  const copyValue = visible ? value : presentationValue(value, kind);
  if (!privacyEnabled() || !value) return <span>{display}</span>;
  return (
    <span className="reveal-field">
      <span>{display}</span>
      <button
        type="button"
        className="chip"
        aria-label={`Copy ${label}`}
        onClick={() => void navigator.clipboard?.writeText(copyValue)}
      >
        copy
      </button>
      <button
        type="button"
        className="chip"
        aria-pressed={visible}
        aria-label={visible ? `Mask ${label}` : `Reveal ${label} for ten seconds`}
        onClick={() => (visible ? setPrivacy(true) : revealField(fieldKey))}
      >
        {visible ? "mask" : "reveal"}
      </button>
    </span>
  );
}
