// Download categories, derived from the file extension on the frontend (the engine stores no category).
import { type Job, extOf } from "./api";

export type FileCategory = "compressed" | "documents" | "music" | "programs" | "video";
export type CategoryId = "all" | FileCategory | "unfinished" | "finished" | "queues";

// One table for the engine (Rust) and the UI: rebuild/shared/categories.json.
import table from "../../../shared/categories.json";

export const FILE_CATEGORIES = table.categories as { id: FileCategory; label: string; ext: string[] }[];

/** Category of a file name, or `null` for "other" (shown only under All). */
export function categoryOf(name: string): FileCategory | null {
  const e = extOf(name);
  if (!e) return null;
  // "archive.tar.zst" style names count as compressed too.
  return FILE_CATEGORIES.find((c) => c.ext.includes(e))?.id ?? null;
}

export function inCategory(job: Job, cat: CategoryId): boolean {
  switch (cat) {
    case "all":
      return true;
    case "unfinished":
      return job.state !== "done";
    case "finished":
      return job.state === "done";
    case "queues":
      return false; // no named queues in the engine yet
    default:
      return categoryOf(job.name) === cat;
  }
}
