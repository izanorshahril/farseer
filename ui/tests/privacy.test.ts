import { describe, expect, test } from "bun:test";
import { isFieldRevealed, mask, presentationValue, privacyEnabled, revealField, setPrivacy } from "../src/privacy";

describe("presentation privacy", () => {
  test("masks sensitive values without changing the source value", () => {
    setPrivacy(true);
    expect(mask("D:\\Private\\project", "path")).toBe("path hidden");
    expect(mask("account@example.test", "account")).toBe("account hidden");
    expect(mask("session-123", "session")).toBe("hidden");
    setPrivacy(false);
    expect(privacyEnabled()).toBe(false);
    expect(mask("D:\\Private\\project", "path")).toBe("D:\\Private\\project");
    setPrivacy(true);
  });

  test("starts screenshot-safe", () => {
    setPrivacy(true);
    expect(privacyEnabled()).toBe(true);
    expect(mask("C:\\work\\farseer", "path")).toBe("path hidden");
    expect(presentationValue("C:\\work\\farseer", "path")).toBe("path hidden");
  });

  test("reveals one field briefly and clears it when privacy is re-enabled", () => {
    setPrivacy(true);
    expect(isFieldRevealed("quota:one")).toBe(false);
    revealField("quota:one", 60_000);
    expect(isFieldRevealed("quota:one")).toBe(true);
    setPrivacy(true);
    expect(isFieldRevealed("quota:one")).toBe(false);
  });
});
