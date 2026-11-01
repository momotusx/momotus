/** The words Versions, the title page and the player never say (plans/versions-web-spec.md 6, plan
 * Step 3.4): the string literals and JSX text of their sources, read through the TypeScript parser so
 * comments and names never match, and every value of the shared stream choice words. */

import { expect, test } from "bun:test";
import { readFileSync } from "node:fs";
import ts from "typescript";
import shared from "../../../shared/stream-choice-words.json";

// Incognito's words too: a full disk at its entry says it could not open, never why (plans/no-space-warnings.md).
const SOURCES = ["screens/versions.tsx", "screens/title.tsx", "screens/player.tsx", "player/loading.tsx", "lib/words.ts", "components/empty-acts.tsx", "components/incognito.tsx", "lib/profiles.ts"];
// No free space or storage warning: only the parts watched are downloaded (the owner, plans/no-space-warnings.md).
const SPACE = [/free space/i, /\{free\}/, /nearly full/i, /device is full/i, /storage full/i, /storage is full/i, /no room/i, /for this storage/i, /not enough (space|room|storage)/i, /out of space/i, /low on storage/i, /free some storage/i];
// No account step off Settings: no link to the accounts screen and no service name (the owner, 2026-10-23).
const BANNED = [/no playable/i, /not found/i, /not measured/i, /add an account/i, /show them/i, /hidden \(/i, /\bvia\b/i, /estimated/i, /\u2014/, /\w--\w/, /^\/debrid\b/, /real-debrid|torbox|alldebrid|premiumize/i, ...SPACE];

// Still said until another step removes them, each named exactly: a new one fails.
// ponytail: the list empties as those steps land; delete each entry with its words.
const PENDING = new Set([
  // The title page's old line, until design-web's title step reads the core's empty state.
  "lib/words.ts: No playable version found",
  // The blocked words, until every app reads `empty` (plan Step 3.6).
  "json blocked.nothingPlaysHere.action: Show them",
  "json blocked.showRows: Show them",
]);

function literals(file: string): string[] {
  const src = ts.createSourceFile(file, readFileSync(new URL(`../${file}`, import.meta.url), "utf8"), ts.ScriptTarget.Latest, true, ts.ScriptKind.TSX);
  const out: string[] = [];
  const walk = (n: ts.Node) => {
    if (ts.isStringLiteralLike(n) || ts.isTemplateHead(n) || ts.isTemplateMiddle(n) || ts.isTemplateTail(n)) out.push(n.text);
    else if (ts.isJsxText(n) && n.text.trim()) out.push(n.text.trim());
    n.forEachChild(walk);
  };
  walk(src);
  return out;
}

function values(v: unknown, path: string, out: [string, string][]) {
  if (typeof v === "string") out.push([path, v]);
  else if (Array.isArray(v)) v.forEach((x, i) => values(x, `${path}[${i}]`, out));
  else if (v && typeof v === "object") for (const [k, x] of Object.entries(v)) values(x, path ? `${path}.${k}` : k, out);
  return out;
}

test("Versions, the title page and the player never say the banned words", () => {
  const said = [...SOURCES.flatMap((f) => literals(f).map((t) => `${f}: ${t}`)), ...values(shared, "", []).filter(([p]) => p !== "about").map(([p, t]) => `json ${p}: ${t}`)];
  const hits = said.filter((s) => BANNED.some((b) => b.test(s.slice(s.indexOf(": ") + 2))));
  expect(hits.filter((h) => !PENDING.has(h))).toEqual([]);
  // A pending entry that is no longer said goes from the list.
  expect([...PENDING].filter((p) => !hits.includes(p))).toEqual([]);
});
