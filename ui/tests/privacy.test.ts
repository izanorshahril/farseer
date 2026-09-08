import { describe, expect, test } from "bun:test";
import {
  copyPresentation,
  exportPresentation,
  isFieldRevealed,
  mask,
  presentationValue,
  privacyEnabled,
  revealField,
  setPrivacy,
} from "../src/privacy";

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

  test("copy/export presentation keeps the source value unchanged", () => {
    const source = "C:\\private\\farseer";
    expect(presentationValue(source, "path", true)).toBe("path hidden");
    expect(presentationValue(source, "path", false)).toBe(source);
    expect(presentationValue(source, "path", true, "path:one")).toBe("path hidden");
    revealField("path:one", 60_000);
    expect(presentationValue(source, "path", true, "path:one")).toBe(source);
    expect(exportPresentation({
      account: { value: "account@example.test", kind: "account" },
      path: { value: source, kind: "path" },
    }, true)).toContain('"path": "path hidden"');
    revealField("account:one", 60_000);
    const authorizedExport = exportPresentation({
      account: { value: "account@example.test", kind: "account", fieldKey: "account:one" },
      path: { value: source, kind: "path", fieldKey: "path:one" },
    }, true);
    expect(JSON.parse(authorizedExport)).toEqual({
      account: "account@example.test",
      path: source,
    });
    expect(source).toBe("C:\\private\\farseer");
  });

  test("copy writes the masked value until a field is explicitly authorized", async () => {
    const writes: string[] = [];
    const prior = globalThis.navigator;
    Object.defineProperty(globalThis, "navigator", {
      configurable: true,
      value: { clipboard: { writeText: (value: string) => { writes.push(value); } } },
    });
    await copyPresentation("account@example.test", "account", true);
    revealField("copy:account", 60_000);
    await copyPresentation("account@example.test", "account", true, "copy:account");
    Object.defineProperty(globalThis, "navigator", { configurable: true, value: prior });
    expect(writes).toEqual(["account hidden", "account@example.test"]);
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
