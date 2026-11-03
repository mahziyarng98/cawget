import { describe, expect, it } from "vitest";
import { categoryOf, inCategory } from "../src/lib/categories";
import { job } from "./fixtures";

describe("categories", () => {
  it("classifies by extension, case-insensitively", () => {
    expect(categoryOf("linux.tar.xz")).toBe("compressed");
    expect(categoryOf("Report.PDF")).toBe("documents");
    expect(categoryOf("song.flac")).toBe("music");
    expect(categoryOf("tool.AppImage")).toBe("programs");
    expect(categoryOf("clip.mkv")).toBe("video");
    expect(categoryOf("noext")).toBeNull();
    expect(categoryOf("data.bin")).toBeNull();
    expect(categoryOf(".hidden")).toBeNull();
  });

  it("filters by state and category; queues are not supported yet", () => {
    const d = job({ state: "done", name: "a.zip" });
    const u = job({ state: "active", name: "b.mp4" });
    expect(inCategory(d, "finished") && !inCategory(u, "finished")).toBe(true);
    expect(inCategory(u, "unfinished") && !inCategory(d, "unfinished")).toBe(true);
    expect(inCategory(d, "compressed") && inCategory(u, "video") && !inCategory(u, "music")).toBe(true);
    expect(inCategory(d, "all")).toBe(true);
    expect(inCategory(d, "queues")).toBe(false);
  });
});
