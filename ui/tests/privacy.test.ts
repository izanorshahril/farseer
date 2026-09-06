import { describe, expect, test } from "bun:test";
import { mask, privacyEnabled, setPrivacy } from "../src/privacy";

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
  });
});
