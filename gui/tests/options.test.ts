import { describe, expect, it } from "vitest";
import { hhmm, localInputToUnix, nextLocalHour, proxyMode, proxySetting, speedSetting, speedToEditor, unixToLocalInput } from "../src/lib/options";

describe("options helpers", () => {
  it("round-trips the speed limit editor", () => {
    expect(speedToEditor(0)).toEqual({ value: 0, unit: "K" });
    expect(speedToEditor(2 * 1048576)).toEqual({ value: 2, unit: "M" });
    expect(speedToEditor(500 * 1024)).toEqual({ value: 500, unit: "K" });
    expect(speedSetting(2, "M")).toBe("2M");
    expect(speedSetting(0, "K")).toBe("");
    expect(speedSetting(Number.NaN, "K")).toBe("");
  });

  it("maps proxy modes and validates manual addresses", () => {
    expect(proxyMode("")).toEqual({ mode: "system", url: "" });
    expect(proxyMode("none")).toEqual({ mode: "none", url: "" });
    expect(proxyMode("http://p:3128")).toEqual({ mode: "manual", url: "http://p:3128" });
    expect(proxySetting("system", "x")).toBe("");
    expect(proxySetting("none", "x")).toBe("none");
    expect(proxySetting("manual", " http://proxy.lan:3128 ")).toBe("http://proxy.lan:3128");
    expect(proxySetting("manual", "socks5://127.0.0.1:1080")).toBe("socks5://127.0.0.1:1080");
    expect(proxySetting("manual", "http://user:pw@proxy:8080")).toBe("http://user:pw@proxy:8080");
    expect(proxySetting("manual", "proxy:3128")).toBeNull();
    expect(proxySetting("manual", "ftp://p:21")).toBeNull();
  });

  it("formats times and converts schedule inputs", () => {
    expect(hhmm(0)).toBe("00:00");
    expect(hhmm(390)).toBe("06:30");
    const t = localInputToUnix("2030-01-02T03:04")!;
    expect(unixToLocalInput(t)).toBe("2030-01-02T03:04");
    expect(localInputToUnix("")).toBeNull();
    expect(localInputToUnix("tomorrow")).toBeNull();
  });
  it("defaults the start time to the next full local hour", () => {
    const now = Date.now() / 1000;
    const t = nextLocalHour(now);
    expect(t).toBeGreaterThan(now);
    expect(t - now).toBeLessThanOrEqual(3600);
    const d = new Date(t * 1000);
    expect([d.getMinutes(), d.getSeconds()]).toEqual([0, 0]);
  });
});
