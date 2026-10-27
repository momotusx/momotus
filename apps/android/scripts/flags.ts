// The subtitle picker's language flags (plans/player-polish.md 2.4): HatScripts circle-flags (MIT), its
// language set, as Android vector drawables in the player module, and the table from a language tag to each.
//
//   curl -sSfL -o cf.tgz https://registry.npmjs.org/circle-flags/-/circle-flags-2.8.3.tgz
//   mkdir cf && tar -xzf cf.tgz -C cf
//   bun scripts/flags.ts cf/package
//
// Every flag is a 512 square under one circle mask: the mask becomes the group's clip path, and each shape a
// path (circles, ellipses and rounded rects drawn as arcs). Path data is read and written again with plain
// separators, since the minified arcs pack their flags ("a8 8 0 011 1"), which Android's parser misreads.
import { existsSync, mkdirSync, readFileSync, readdirSync, rmSync, writeFileSync } from "node:fs";
import { join } from "node:path";

const VERSION = "2.8.3";
const pkg = process.argv[2];
if (!pkg) throw new Error("usage: bun scripts/flags.ts <circle-flags package dir>");
const version = JSON.parse(readFileSync(join(pkg, "package.json"), "utf8")).version;
if (version !== VERSION) throw new Error(`circle-flags ${version}, expected ${VERSION}`);

const here = join(import.meta.dir, "..", "player");
const res = join(here, "src", "main", "res", "drawable");
const MASK = '<mask id="a"><circle cx="256" cy="256" r="256" fill="#fff"/></mask><g mask="url(#a)">';
// Language tags only: the question mark (xx) and the two constructed sets have no tag a track carries.
const TAG = /^(?!xx$)[a-z]{2,3}(-[a-z0-9]{2,4})?$/;
// The languages drawn with the neutral language glyph (design/flags.ts writes the list beside the overlay).
const glyphs: string[] = existsSync(join(pkg, "glyph.json")) ? JSON.parse(readFileSync(join(pkg, "glyph.json"), "utf8")) : [];

const ARITY: Record<string, number> = { m: 2, l: 2, h: 1, v: 1, c: 6, s: 4, q: 4, t: 2, a: 7, z: 0 };

/** The path data again with every number on its own, so no parser has to split packed arc flags. */
const normalize = (d: string) => {
  const out: string[] = [];
  let i = 0;
  const skip = () => {
    while (i < d.length && /[\s,]/.test(d[i]!)) i++;
  };
  const number = () => {
    skip();
    const m = /^[-+]?(?:\d*\.\d+|\d+\.?)(?:[eE][-+]?\d+)?/.exec(d.slice(i));
    if (!m) throw new Error(`no number at ${i} in ${d}`);
    i += m[0].length;
    return m[0];
  };
  const flag = () => {
    skip();
    const f = d[i++];
    if (f !== "0" && f !== "1") throw new Error(`bad arc flag at ${i - 1} in ${d}`);
    return f;
  };
  let cmd = "";
  while (true) {
    skip();
    if (i >= d.length) break;
    if (/[a-zA-Z]/.test(d[i]!)) {
      cmd = d[i++]!;
      if (!(cmd.toLowerCase() in ARITY)) throw new Error(`unknown command ${cmd}`);
      out.push(cmd);
      if (cmd.toLowerCase() === "z") continue;
    } else if (!cmd || cmd.toLowerCase() === "z") {
      throw new Error(`numbers with no command at ${i} in ${d}`);
    }
    const n = ARITY[cmd.toLowerCase()]!;
    const params = [];
    for (let k = 0; k < n; k++) params.push(cmd.toLowerCase() === "a" && (k === 3 || k === 4) ? flag() : number());
    out.push(params.join(" "));
  }
  return out.join(" ").replace(/([a-zA-Z]) /g, "$1");
};

const attrs = (tag: string) => Object.fromEntries([...tag.matchAll(/([a-z-]+)="([^"]*)"/g)].map((m) => [m[1]!, m[2]!]));
const color = (fill: string) => {
  const hex = fill.replace("#", "");
  if (!/^([0-9a-f]{3}|[0-9a-f]{6})$/i.test(hex)) throw new Error(`fill ${fill}`);
  return `#${(hex.length === 3 ? [...hex].map((c) => c + c).join("") : hex).toUpperCase()}`;
};
const n = (v: string | undefined) => Number(v ?? 0);
// An ellipse as two half arcs; a circle is one with rx = ry.
const ellipse = (cx: number, cy: number, rx: number, ry: number) =>
  `M${cx - rx} ${cy}a${rx} ${ry} 0 1 0 ${2 * rx} 0a${rx} ${ry} 0 1 0 ${-2 * rx} 0z`;
const rect = (x: number, y: number, w: number, h: number, r: number) =>
  r <= 0
    ? `M${x} ${y}h${w}v${h}h${-w}z`
    : `M${x + r} ${y}h${w - 2 * r}a${r} ${r} 0 0 1 ${r} ${r}v${h - 2 * r}a${r} ${r} 0 0 1 ${-r} ${r}h${-(w - 2 * r)}a${r} ${r} 0 0 1 ${-r} ${-r}v${-(h - 2 * r)}a${r} ${r} 0 0 1 ${r} ${-r}z`;

/** One shape of a flag as a path, or null for one that draws nothing (fill none, no stroke). */
const shape = (tag: string): string | null => {
  const name = /^<([a-z]+)/.exec(tag)![1]!;
  const a = attrs(tag);
  if (a.fill === "none") return null;
  if (a.transform) throw new Error(`transform ${a.transform}`);
  const d =
    name === "path"
      ? a.d!
      : name === "circle"
        ? ellipse(n(a.cx), n(a.cy), n(a.r), n(a.r))
        : name === "ellipse"
          ? ellipse(n(a.cx), n(a.cy), n(a.rx), n(a.ry))
          : name === "rect"
            ? rect(n(a.x), n(a.y), n(a.width), n(a.height), Math.min(n(a.rx ?? a.ry), n(a.width) / 2, n(a.height) / 2))
            : (() => {
                throw new Error(`element ${name}`);
              })();
  return `        <path\n            android:fillColor="${color(a.fill ?? "#000")}"\n            android:pathData="${normalize(d)}" />`;
};

const vector = (file: string, svg: string) => {
  if (!svg.includes(MASK)) throw new Error(`${file}: not one circle mask`);
  const body = svg.slice(svg.indexOf(MASK) + MASK.length, svg.lastIndexOf("</g>"));
  const shapes = [...body.matchAll(/<[a-z]+\b[^>]*\/>/g)].map((m) => shape(m[0])).filter(Boolean);
  if (body.replace(/<[a-z]+\b[^>]*\/>/g, "").trim()) throw new Error(`${file}: more than flat shapes`);
  return `<?xml version="1.0" encoding="utf-8"?>
<!-- circle-flags ${VERSION} (MIT, HatScripts), flags/language/${file}; written by scripts/flags.ts -->
<vector xmlns:android="http://schemas.android.com/apk/res/android"
    android:width="24dp"
    android:height="24dp"
    android:viewportWidth="512"
    android:viewportHeight="512">
    <group>
        <clip-path android:pathData="${normalize(ellipse(256, 256, 256, 256))}" />
${shapes.join("\n")}
    </group>
</vector>
`;
};

const dir = join(pkg, "flags", "language");
const tags = readdirSync(dir)
  .filter((f) => f.endsWith(".svg"))
  .map((f) => f.slice(0, -4))
  .filter((t) => TAG.test(t))
  .sort();
mkdirSync(res, { recursive: true });
for (const old of readdirSync(res).filter((f) => f.startsWith("flag_"))) rmSync(join(res, old));
for (const tag of tags) writeFileSync(join(res, `flag_${tag.replace("-", "_")}.xml`), vector(`${tag}.svg`, readFileSync(join(dir, `${tag}.svg`), "utf8")));

const kotlin = `package app.marquee.player

// circle-flags ${VERSION} (MIT, HatScripts): its language set, by language tag. Written by scripts/flags.ts.
internal val FLAGS: Map<String, Int> =
    mapOf(
${tags.map((t) => `        "${t}" to R.drawable.flag_${t.replace("-", "_")},`).join("\n")}
    )

/** The languages drawn with the neutral language glyph, not a flag (design/flag-overrides.json). */
internal val GLYPHS: Set<String> =
    setOf(
${glyphs.map((t) => `        "${t}",`).join("\n")}
    )
`;
writeFileSync(join(here, "src", "main", "kotlin", "app", "marquee", "player", "Flags.kt"), kotlin);
// MIT asks for the notice in every copy: it ships in the APK beside the flags.
const licenses = join(here, "src", "main", "assets", "licenses");
mkdirSync(licenses, { recursive: true });
writeFileSync(join(licenses, "circle-flags-MIT.md"), readFileSync(join(pkg, "LICENSE.md"), "utf8"));
console.log(`${tags.length} flags from circle-flags ${VERSION}`);
