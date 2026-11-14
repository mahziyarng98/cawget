import { describe, expect, it } from "vitest";
import { isThemeMode, mediaPrefersDark, resolveTheme, watchMediaTheme } from "../src/lib/theme";

describe("theme mode", () => {
  it("accepts only the three stored preferences", () => {
    expect(["system", "light", "dark"].every(isThemeMode)).toBe(true);
    for (const bad of ["", "auto", "Dark", null, 0, {}]) expect(isThemeMode(bad)).toBe(false);
  });

  it("follows the desktop in System mode and otherwise uses the explicit choice", () => {
    expect(resolveTheme("system", false)).toBe("light");
    expect(resolveTheme("system", true)).toBe("dark");
    expect(resolveTheme("light", true)).toBe("light");
    expect(resolveTheme("dark", false)).toBe("dark");
  });

  it("defaults to light where no browser media query exists", () => {
    expect(mediaPrefersDark()).toBe(false);
    expect(watchMediaTheme(() => {})).toBeTypeOf("function");
    expect(() => watchMediaTheme(() => {})()).not.toThrow();
  });
});
