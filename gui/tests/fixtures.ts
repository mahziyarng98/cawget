import type { Job } from "../src/lib/api";

let next = 1;
export function job(p: Partial<Job> = {}): Job {
  const id = p.id ?? next++;
  return {
    id, url: `http://h/${id}`, out_path: "/tmp/", state: "queued", error: null, size: null, accept_ranges: false,
    etag: null, last_modified: null, sha256: null, referer: null, headers: [], segments: 0, done: 0, speed: 0,
    connections: 0, fraction: null, eta_secs: null, name: `file${id}.bin`, not_before: 0, created: 0, parts: [], ...p,
  };
}
