import { describe, expect, it } from "vitest";
import { authError, joinPath, parseUrls, saveTarget, splitPath } from "../src/lib/addflow";

describe("add flow helpers", () => {
  it("extracts http(s) addresses, drops duplicates and other text", () => {
    expect(parseUrls("  https://a/x  \nftp://b/y\nhttp://c/z https://a/x junk")).toEqual(["https://a/x", "http://c/z"]);
    expect(parseUrls("")).toEqual([]);
  });

  it("validates the optional authorization", () => {
    expect(authError(false, { login: "", password: "" })).toBeNull();
    expect(authError(true, { login: "", password: "x" })).toMatch(/user name/);
    expect(authError(true, { login: "a:b", password: "" })).toMatch(/colon/);
    expect(authError(true, { login: "user", password: "" })).toBeNull();
  });

  it("joins and splits paths", () => {
    expect(joinPath("/home/u/Downloads", "a.iso")).toBe("/home/u/Downloads/a.iso");
    expect(joinPath("/home/u/Downloads/", "a.iso")).toBe("/home/u/Downloads/a.iso");
    expect(splitPath("/home/u/a.iso")).toEqual({ dir: "/home/u", name: "a.iso" });
    expect(splitPath("/a.iso")).toEqual({ dir: "/", name: "a.iso" });
    expect(splitPath("a.iso")).toEqual({ dir: "", name: "a.iso" });
  });

  it("maps Save as to a folder or an exact file", () => {
    expect(saveTarget("/home/u/Downloads/")).toEqual({ dir: "/home/u/Downloads/" });
    expect(saveTarget(" /home/u/a.iso ")).toEqual({ output: "/home/u/a.iso" });
    expect(saveTarget("")).toEqual({});
  });
});
