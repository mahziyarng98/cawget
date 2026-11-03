// Pure helpers for the Add downloads -> File details flow (unit-tested).

/** http(s) addresses in a block of text, in order, without duplicates. */
export function parseUrls(text: string): string[] {
  const seen = new Set<string>();
  const out: string[] = [];
  for (const t of text.split(/\s+/)) {
    if (/^https?:\/\/\S+$/i.test(t) && !seen.has(t)) {
      seen.add(t);
      out.push(t);
    }
  }
  return out;
}

export interface Auth {
  login: string;
  password: string;
}

/** Problem with the optional authorization, or null. A login may not contain ":" (RFC 7617). */
export function authError(use: boolean, a: Auth): string | null {
  if (!use) return null;
  if (!a.login) return "Enter a user name, or turn off logging in";
  if (a.login.includes(":")) return "A user name cannot contain a colon (:)";
  return null;
}

/** Joins a folder and a file name with exactly one slash. */
export function joinPath(dir: string, name: string): string {
  if (!dir) return name;
  return dir.endsWith("/") ? dir + name : `${dir}/${name}`;
}

/** Splits a path into folder (without trailing slash) and file name. */
export function splitPath(path: string): { dir: string; name: string } {
  const i = path.lastIndexOf("/");
  if (i < 0) return { dir: "", name: path };
  return { dir: i === 0 ? "/" : path.slice(0, i), name: path.slice(i + 1) };
}

/** How the Save as field maps to the engine: a folder (name chosen by the engine) or an exact file. */
export function saveTarget(saveAs: string): { dir?: string; output?: string } {
  const p = saveAs.trim();
  if (!p) return {};
  if (p.endsWith("/")) return { dir: p };
  return { output: p };
}
