/** The LG webOS build (plans/webos.md section 2): `vite build --mode webos` writes `dist-webos/` for Chromium 79,
 * served by the app's own service at http://127.0.0.1:47801/. The browser and desktop builds never load this file's
 * CSS work: their transformer and output stay as they are. */

import { readdirSync, readFileSync } from "node:fs";
import { join } from "node:path";
import { type Declaration, type Rule, type SelectorComponent, type StyleRule, transform } from "lightningcss";
import type { Plugin, UserConfig } from "vite";

/** Chromium 79 in Lightning CSS's version form (major << 16). */
const CHROME_79 = 79 << 16;

const SRC = join(import.meta.dirname, "../src");

/** The `webos` mode on top of the shared config. */
export function webos(config: UserConfig): UserConfig {
  return {
    ...config,
    base: "/",
    plugins: [...(config.plugins ?? []), webosPage()],
    css: { transformer: "lightningcss", lightningcss: { targets: { chrome: CHROME_79 }, visitor: chromium79Visitor() } },
    build: { ...config.build, target: "chrome79", cssMinify: "lightningcss", outDir: "dist-webos", emptyOutDir: true },
  };
}

/** Sets `no-flex-gap` on <html> before the first paint where flex boxes have no `gap` (Chromium below 84): a column
 * flex box with `gap: 1px` and two empty children is 1 px tall only where flex gap works. */
export const FLEX_GAP_PROBE =
  "<script>(function(){var d=document,f=d.createElement('div');f.style.cssText='display:flex;flex-direction:column;row-gap:1px;position:absolute';" +
  "f.appendChild(d.createElement('div'));f.appendChild(d.createElement('div'));d.documentElement.appendChild(f);" +
  "if(f.scrollHeight!==1)d.documentElement.classList.add('no-flex-gap');f.remove()})()</script>";

/** The page's webOS half: the flex gap probe in <head>, and `theme/webos.css` (the rules written by hand for
 * Chromium 79) right after `theme/app.css`, so it holds app.css's place in the cascade. */
function webosPage(): Plugin {
  return {
    name: "momotus-webos-page",
    transformIndexHtml: (html) => html.replace("</head>", `    ${FLEX_GAP_PROBE}\n  </head>`),
    transform(code, id) {
      if (!id.endsWith("/src/main.tsx")) return;
      const app = 'import "./theme/app.css";';
      if (!code.includes(app)) throw new Error("main.tsx no longer imports ./theme/app.css: place theme/webos.css after it");
      return code.replace(app, `${app}\nimport "./theme/webos.css";`);
    },
  };
}

// ---- Lightning CSS AST helpers -------------------------------------------------------------------------------------

type Sel = SelectorComponent[];
type Decls = { declarations?: Declaration[]; importantDeclarations?: Declaration[] };

/** Lightning reads back a null optional (a `var()`'s `from` and `fallback`) as an error: drop every null key before
 * any AST goes back, which it reads as absent. */
function clean<T>(value: T): T {
  if (Array.isArray(value)) value.forEach(clean);
  else if (value && typeof value === "object") {
    const bag = value as Record<string, unknown>;
    for (const key of Object.keys(bag)) if (bag[key] === null) delete bag[key];
    Object.values(bag).forEach(clean);
  }
  return value;
}

/** Prints one declaration as `property: value` through Lightning itself. */
function printDecl(decl: Declaration): string {
  const code = transform({
    filename: "decl.css",
    code: Buffer.from("a{color:red}"),
    visitor: {
      Rule: {
        style(rule) {
          rule.value.declarations = { declarations: [clean(structuredClone(decl))], importantDeclarations: [] };
          return rule;
        },
      },
    },
  }).code.toString();
  return /\{\s*([\s\S]*?);?\s*\}/.exec(code)?.[1]?.trim() ?? "";
}

/** Prints a selector through Lightning itself. */
function printSel(sel: Sel): string {
  const code = transform({
    filename: "sel.css",
    code: Buffer.from("a{color:red}"),
    visitor: {
      Rule: {
        style(rule) {
          rule.value.selectors = [clean(structuredClone(sel))];
          return rule;
        },
      },
    },
  }).code.toString();
  return code.slice(0, code.indexOf("{")).trim();
}

/** Parses `text` (declarations) into Lightning's AST. */
function parseDecls(text: string): Declaration[] {
  let out: Declaration[] = [];
  transform({
    filename: "parse.css",
    code: Buffer.from(`a{${text}}`),
    visitor: {
      Rule: {
        style(rule) {
          out = rule.value.declarations?.declarations ?? [];
        },
      },
    },
  });
  return clean(out);
}

/** Splits a value at its top-level spaces: `0 calc(-1 * var(--x))` is two parts. */
function words(value: string): string[] {
  const out: string[] = [];
  let depth = 0;
  let part = "";
  for (const ch of value.trim()) {
    if (ch === "(") depth++;
    if (ch === ")") depth--;
    if (ch === " " && depth === 0) {
      if (part) out.push(part);
      part = "";
    } else part += ch;
  }
  if (part) out.push(part);
  return out;
}

// ---- Selectors ----------------------------------------------------------------------------------------------------

const isComb = (c: SelectorComponent) => c.type === "combinator";
/** Where the last compound starts. */
const afterLastComb = (sel: Sel) => sel.map(isComb).lastIndexOf(true) + 1;
const pseudo = (c: SelectorComponent, kind: string): c is Extract<SelectorComponent, { type: "pseudo-class" }> & { selectors: Sel[] } =>
  c.type === "pseudo-class" && (c as { kind?: string }).kind === kind;

/** Selector specificity as [ids, classes, types]. */
function specificity(sel: Sel): [number, number, number] {
  const s: [number, number, number] = [0, 0, 0];
  const add = (o: [number, number, number]) => o.forEach((n, i) => (s[i as 0 | 1 | 2] += n));
  for (const c of sel) {
    const kind = (c as { kind?: string }).kind;
    const args = (c as { selectors?: Sel[] }).selectors;
    if (c.type === "id") s[0]++;
    else if (c.type === "class" || c.type === "attribute") s[1]++;
    else if (c.type === "type" || c.type === "pseudo-element") s[2]++;
    else if (c.type === "pseudo-class") {
      if (kind === "where") continue;
      if (args && ["is", "not", "has", "any"].includes(kind ?? "")) add(args.map(specificity).reduce(maxSpec, [0, 0, 0]));
      else s[1]++;
    }
  }
  return s;
}
const maxSpec = (a: [number, number, number], b: [number, number, number]) => (a[0] !== b[0] ? (a[0] > b[0] ? a : b) : a[1] !== b[1] ? (a[1] > b[1] ? a : b) : a[2] >= b[2] ? a : b);

/** Rules the TV form reaches with an `:is()` alternative heavier in one part than the `:is()` as a whole (`:is(.own, small)`: [0,0,1]
 * against [0,1,0]). Padding only adds, so it comes out one part heavier there than on a newer engine; each is decided
 * here, with why that changes nothing, and any other fails the build. */
export const OVERWEIGHT: Record<string, string> = {
  '[data-form="tv"] .subp .opt.on :is(.own, .ct, small, .sep)': "its small at 0,5,1 for 0,5,0: the only color rules on it at 0,5,x are this and the :focus one, both padded alike",
  '[data-form="tv"] .subp .opt:focus :is(.own, .ct, small, .ck, .sep)': "as the .on one above, which it still follows",
  '[data-form="tv"] .first .ptile:focus :is(.pname, small)': "its small at 0,5,1 for 0,5,0: no other color rule on a .ptile small is above 0,2,1",
  ".fr-chips .chip :is(.flag, svg)": "its svg at 0,3,1 for 0,3,0: the only other size rule on it is .chip svg, 0,1,1",
  '[data-form="tv"] .first :is(.btn, .flink, .fr-opt, .seg button, .fr-chips .chip, .ptile, .wall-pause, .swx button, .fr-field input):focus':
    "its .seg button, .swx button and .fr-field input at 0,5,1 for 0,5,0: the rules on them stop at 0,4,1 (the field's own :focus), which it beats either way",
};

/** `:not(...)` arguments that never match, to raise an expanded alternative to its `:is()`'s specificity (`rule`, the
 * selector it came from, names a mixed case: see OVERWEIGHT). */
function padding(alt: Sel, want: [number, number, number], rule: Sel): Sel {
  const have = specificity(alt);
  if (have.some((n, i) => n > want[i]!) && !(printSel(rule) in OVERWEIGHT))
    throw new Error(`${printSel(rule)}: the :is() alternative ${printSel(alt)} (specificity ${have}) cannot be padded to ${want} on Chromium 79. Rewrite it, or list it in OVERWEIGHT (scripts/webos-build.ts) with why the extra weight changes nothing.`);
  const pad: Sel = [];
  const not = (c: SelectorComponent) => pad.push({ type: "pseudo-class", kind: "not", selectors: [[c]] } as SelectorComponent);
  for (let i = have[0]; i < want[0]; i++) not({ type: "id", name: "mq-pad" });
  for (let i = have[1]; i < want[1]; i++) not({ type: "class", name: "mq-pad" });
  for (let i = have[2]; i < want[2]; i++) not({ type: "type", name: "mq-pad" } as SelectorComponent);
  return pad;
}

/** A compound's type or universal selector goes first (`.a` + `div` is `div.a`). */
function tidyCompounds(sel: Sel): Sel {
  const out: Sel = [];
  let compound: Sel = [];
  const flush = () => {
    out.push(...compound.filter((c) => c.type === "type" || c.type === "universal"), ...compound.filter((c) => c.type !== "type" && c.type !== "universal"));
    compound = [];
  };
  for (const c of sel) {
    if (isComb(c)) {
      flush();
      out.push(c);
    } else compound.push(c);
  }
  flush();
  return out;
}

const RTL = ["ae", "ar", "arc", "bcc", "bqi", "ckb", "dv", "fa", "glk", "he", "ku", "mzn", "nqo", "pnb", "ps", "sd", "ug", "ur", "yi"];

/** Every `:is()` and `:where()` written out as a list of plain selectors (Chromium 79 reads neither). An `:is()`
 * keeps its specificity: a lower alternative is padded with `:not()`s that never match. Null when one cannot be
 * written out (a complex alternative inside a compound). */
function expand(sel: Sel, rule: Sel = sel): Sel[] | null {
  const dir = sel.findIndex((c) => pseudo(c, "dir"));
  if (dir >= 0) {
    // `:dir()` (Chrome 120) as the right-to-left languages, the list Lightning CSS lowers it to.
    const lang = RTL.map((l) => [{ type: "pseudo-class", kind: "lang", languages: [l] }] as Sel);
    const rtl = (sel[dir] as { direction?: string }).direction === "rtl";
    const swap: Sel = rtl ? [{ type: "pseudo-class", kind: "is", selectors: lang } as SelectorComponent] : lang.map((l) => ({ type: "pseudo-class", kind: "not", selectors: [l] }) as SelectorComponent);
    return expand([...sel.slice(0, dir), ...swap, ...sel.slice(dir + 1)], rule);
  }
  const i = sel.findIndex((c) => pseudo(c, "is") || pseudo(c, "where"));
  if (i < 0) return [sel];
  const c = sel[i] as { kind: string; selectors: Sel[] };
  // The compound around it joins each alternative's last compound: `.x:is(.a .b):focus` is `.a .b.x:focus`.
  // ponytail: an alternative with its own ancestors puts them after the selector's earlier compounds (`P :is(A B)`
  // as `P A B`), which holds wherever A sits inside P, as it does in this app's markup.
  const start = afterLastComb(sel.slice(0, i));
  const want = c.selectors.map(specificity).reduce(maxSpec, [0, 0, 0]);
  const out: Sel[] = [];
  for (const alt of c.selectors) {
    const pad = c.kind === "is" ? padding(alt, want, rule) : [];
    const more = expand(tidyCompounds([...sel.slice(0, start), ...alt, ...sel.slice(start, i), ...pad, ...sel.slice(i + 1)]), rule);
    if (!more) return null;
    out.push(...more);
  }
  return out;
}

/** `:focus-visible` is `:focus` on a TV (the D-pad is the only pointer that focuses), and `:not(a, b)` is
 * `:not(a):not(b)`. */
function lower(sel: Sel): Sel {
  const out: Sel = [];
  for (const c of sel) {
    if (pseudo(c, "focus-visible")) out.push({ type: "pseudo-class", kind: "focus" } as SelectorComponent);
    else if (pseudo(c, "not")) for (const arg of c.selectors) out.push({ type: "pseudo-class", kind: "not", selectors: [lower(arg)] } as SelectorComponent);
    else out.push(c);
  }
  return out;
}

const SCOPED_AWAY = new Set(["phone", "desktop"]);
/** A selector the TV form never matches: its first compound asks for the phone or desktop form. */
function otherForm(sel: Sel): boolean {
  for (const c of sel) {
    if (isComb(c)) return false;
    const a = c as { type: string; name?: string; operation?: { value?: string } };
    if (a.type === "attribute" && a.name === "data-form" && SCOPED_AWAY.has(a.operation?.value ?? "")) return true;
  }
  return false;
}

const UNREAD_PSEUDO = new Set(["has", "is", "where", "focus-visible", "dir", "user-valid", "user-invalid", "modal", "popover-open", "autofill"]);
/** What Chromium 79 cannot read in a selector, or null: a newer pseudo-class, or a `:not()` of more than one simple selector. */
function unreadable(sel: Sel): string | null {
  for (const c of sel) {
    const kind = (c as { kind?: string }).kind ?? "";
    if (c.type === "pseudo-class" && UNREAD_PSEUDO.has(kind)) return `:${kind}`;
    if (pseudo(c, "not")) {
      const [arg, ...rest] = c.selectors;
      if (rest.length || !arg || arg.length !== 1 || isComb(arg[0]!)) return ":not() of a complex selector";
      const inner = unreadable(arg);
      if (inner) return inner;
    }
  }
  return null;
}

/** Selectors the TV form reaches that Chromium 79 cannot read, each with what stands in for it there. Anything not
 * listed fails the build, so a new one is decided, not lost. */
export const KNOWN_LOSSES: Record<string, string> = {
  '[data-form="tv"] .btn:not(.info .acts .btn):focus': "theme/webos.css: the white lift on .btn, the title's round actions set back",
  '[data-form="tv"] .info:has(.acts :focus) .play .btn.primary:not(:focus)': "theme/webos.css: .info:focus-within",
  ".tile .pic:has( > img.fade:not(.in))": "the tile's loading pulse: a still placeholder on Chromium 79",
  ".ep-still:has( > img.fade:not(.in))": "the episode still's loading pulse: a still placeholder on Chromium 79",
  ".prefs > .chips-row:has( + .learned)": "the Learned list's top rule stays on Chromium 79",
};

/** Every `aspect-ratio` (Chrome 88) the TV form reaches, by selector, with what stands in for it on Chromium 79. The
 * build drops the line and fails on one not listed, so a new one is decided, not lost. */
export const ASPECT_RATIO_STANDINS: Record<string, string> = {
  ".wall-tile": "theme/webos.css: a ::before as tall as 3:2 of the tile's width",
  ".trailer-frame": "theme/webos.css: a ::before as tall as 9:16 of the frame's width",
  ".logo-slot .title-logo.logo": "the logo's own shape: --r is the trimmed image's ratio, set with its source (art.tsx)",
  ".sync-scan video": "never shown: Chromium 79 has no BarcodeDetector, so canScan() is false (sync.tsx)",
};

const isAspectRatio = (d: Declaration) => d.property === "aspect-ratio" || (d.property === "unparsed" && d.value.propertyId.property === "aspect-ratio");

// ---- Declarations -------------------------------------------------------------------------------------------------

type Rgba = [number, number, number, number];

/** The page colors from theme/tokens.css's first `:root` block, as `--name` to text. */
function rootTokens(): Map<string, string> {
  const css = readFileSync(join(SRC, "theme/tokens.css"), "utf8");
  const root = /:root\s*\{([\s\S]*?)\}/.exec(css)?.[1] ?? "";
  const app = readFileSync(join(SRC, "theme/app.css"), "utf8");
  const appRoot = /:root\s*\{([\s\S]*?)\}/.exec(app)?.[1] ?? "";
  const out = new Map<string, string>();
  for (const block of [root, appRoot]) for (const m of block.matchAll(/(--[\w-]+)\s*:\s*([^;]+);/g)) out.set(m[1]!, m[2]!.trim());
  return out;
}

function parseColor(text: string, tokens: Map<string, string>, depth = 0): Rgba {
  const t = text.trim();
  const v = /^var\((--[\w-]+)\)$/.exec(t);
  if (v) {
    const value = tokens.get(v[1]!);
    if (!value || depth > 5) throw new Error(`color-mix: ${v[1]} has no plain color in theme/tokens.css`);
    return parseColor(value, tokens, depth + 1);
  }
  if (t === "transparent") return [0, 0, 0, 0];
  const hex = /^#([0-9a-f]{3,8})$/i.exec(t)?.[1];
  if (hex) {
    const full = hex.length <= 4 ? [...hex].map((h) => h + h).join("") : hex;
    const n = (i: number) => parseInt(full.slice(i, i + 2), 16);
    return [n(0), n(2), n(4), full.length === 8 ? n(6) / 255 : 1];
  }
  const rgb = /^rgba?\(([^)]*)\)$/.exec(t)?.[1];
  if (rgb) {
    const p = rgb.split(/[\s,/]+/).filter(Boolean).map(Number);
    return [p[0]!, p[1]!, p[2]!, p[3] ?? 1];
  }
  if (t === "white") return [255, 255, 255, 1];
  if (t === "black") return [0, 0, 0, 1];
  throw new Error(`color-mix: cannot read the color ${t}`);
}

/** `color-mix(in srgb, A p%, B [q%])` as one rgba color, premultiplied as the spec mixes. */
function mixColor(args: string, tokens: Map<string, string>): string {
  const [space, a, b] = args.split(/,(?![^(]*\))/).map((s) => s.trim());
  if (space !== "in srgb" || !a || !b) throw new Error(`color-mix: only srgb with two colors: ${args}`);
  const side = (s: string): [Rgba, number | null] => {
    const m = /^(.*?)\s+(-?[\d.]+)%$/.exec(s);
    return m ? [parseColor(m[1]!, tokens), Number(m[2]) / 100] : [parseColor(s, tokens), null];
  };
  const [ca, pa] = side(a);
  const [cb, pb] = side(b);
  const wa = pa ?? (pb === null ? 0.5 : 1 - pb);
  const wb = pb ?? 1 - wa;
  const alpha = ca[3] * wa + cb[3] * wb;
  const ch = (i: number) => (alpha === 0 ? 0 : Math.round((ca[i]! * ca[3] * wa + cb[i]! * cb[3] * wb) / alpha));
  return `rgba(${ch(0)}, ${ch(1)}, ${ch(2)}, ${Math.round(alpha * 1000) / 1000})`;
}

/** One declaration's text rewritten for Chromium 79; the same text when nothing applies. */
export function lowerDeclText(text: string, tokens: Map<string, string>): string {
  let out = text;
  // subgrid (Chrome 117): Chromium 79 drops the line, and so does this build.
  if (/^grid-template-(columns|rows):\s*subgrid/.test(out)) return "";
  // aspect-ratio (Chrome 88): dropped, each one stood in for by ASPECT_RATIO_STANDINS.
  if (/^aspect-ratio:/.test(out)) return "";
  // `color-mix()` (Chrome 111) over the page's own colors: the color it gives on the page, as plain rgba. A plain
  // fallback before it would not do: a declaration with var() is valid until computed, so it always wins over one.
  out = out.replace(/color-mix\(((?:[^()]|\([^()]*\))*)\)/g, (_, args: string) => mixColor(args, tokens));
  // dvh, svh and lvh (Chrome 108): a TV's viewport never changes, so each is vh.
  out = out.replace(/(\d)[dsl]vh\b/g, "$1vh");
  // overflow: clip (Chrome 90) drops the whole line below it, which stops every D-pad scroller.
  if (/^overflow(-x|-y)?:/.test(out)) out = out.replace(/\bclip\b/g, "hidden");
  // padding-inline and margin-inline (Chrome 87): left and right when both sides are the same.
  const inline = /^(padding|margin)-inline:\s*(.*)$/.exec(out);
  if (inline) {
    const [start, end = start] = words(inline[2]!);
    out = start === end ? `${inline[1]}-left: ${start}; ${inline[1]}-right: ${start}` : `${inline[1]}-inline-start: ${start}; ${inline[1]}-inline-end: ${end}`;
  }
  // A transition of `translate` is one of `transform` here (see `composeTransforms`).
  if (/^transition(-property)?:/.test(out)) out = out.replace(/\btranslate\b(?!\()/g, "transform");
  return out;
}

/** `translate` (Chrome 104) and `transform` combined on Chromium 79, where only `transform` exists: each rule that sets
 * either puts its part in `--mq-tl` or `--mq-t` and sets `transform` to both, so the two still cascade on their own.
 * webos.css resets both on every element, so a parent's never reaches a child. */
function composeTransforms(texts: string[]): string[] {
  const at = texts.findIndex((t) => /^(translate|transform):/.test(t));
  if (at < 0) return texts;
  const out: string[] = [];
  for (const t of texts) {
    const tl = /^translate:\s*(.*)$/.exec(t);
    const tf = /^transform:\s*(.*)$/.exec(t);
    if (tl) {
      const [x = "0", y = "0"] = words(tl[1]!);
      out.push(tl[1] === "none" ? "--mq-tl: translate(0)" : `--mq-tl: translate(${x}, ${y})`);
    } else if (tf) out.push(`--mq-t: ${tf[1] === "none" ? "translate(0)" : tf[1]}`);
    else out.push(t);
  }
  out.splice(at + 1, 0, "transform: var(--mq-tl) var(--mq-t)");
  return out;
}

/** Rewrites one declaration list; null when nothing changed. */
function lowerDecls(decls: Declaration[], tokens: Map<string, string>): Declaration[] | null {
  const texts = decls.map(printDecl);
  const lowered = composeTransforms(texts.map((t) => lowerDeclText(t, tokens)).filter(Boolean));
  if (lowered.length === texts.length && lowered.every((t, i) => t === texts[i])) return null;
  return parseDecls(lowered.join(";"));
}

// ---- Flex gap -----------------------------------------------------------------------------------------------------

type Flex = { display?: string; direction?: string; wrap?: string; gap?: { row?: string; column?: string } };

/** The flex facts a rule states itself. */
function flexFacts(texts: string[]): Flex {
  const f: Flex = {};
  for (const t of texts) {
    const [prop, value = ""] = t.split(/:\s*/, 2);
    if (prop === "display") f.display = value;
    if (prop === "flex-direction") f.direction = value;
    if (prop === "flex-wrap") f.wrap = value;
    if (prop === "gap") {
      const [row, column = row] = words(value);
      f.gap = { row, column };
    }
    if (prop === "row-gap") f.gap = { ...f.gap, row: value };
    if (prop === "column-gap") f.gap = { ...f.gap, column: value };
    if (prop === "flex-flow")
      for (const w of words(value)) {
        if (w.includes("wrap")) f.wrap = w;
        else f.direction = w;
      }
  }
  return f;
}

/** Flex facts across src/, for rules that set a gap but take their display from another rule: by one-class selector
 * (`.btn` is an inline flex row) and by whole selector (`.hero .meta`, a TV rule's `[data-form="tv"]` left off). */
function classFacts(): Map<string, Flex> {
  const out = new Map<string, Flex>();
  const files = (dir: string): string[] => readdirSync(dir, { withFileTypes: true }).flatMap((e) => (e.isDirectory() ? files(join(dir, e.name)) : e.name.endsWith(".css") ? [join(dir, e.name)] : []));
  for (const file of files(SRC)) {
    transform({
      filename: file,
      code: readFileSync(file),
      visitor: {
        Rule: {
          style(rule) {
            const facts = flexFacts((rule.value.declarations?.declarations ?? []).map(printDecl));
            for (const sel of rule.value.selectors) {
              const only = sel.length === 1 && sel[0]!.type === "class" ? (sel[0] as { name: string }).name : null;
              if (only) out.set(only, { ...out.get(only), ...facts });
              const whole = bySelector(printSel(sel));
              out.set(whole, { ...out.get(whole), ...facts });
            }
          },
        },
      },
    });
  }
  return out;
}

/** A selector's key in the facts: the TV form's own prefix left off. */
const bySelector = (printed: string) => `sel:${printed.replace(/^\[data-form="tv"\] /, "")}`;

/** Classes that always sit beside another in the markup, whose display they take: decided by hand, each from its TSX. */
const SITS_WITH: Record<string, string> = {
  "big-state": "st", // import.tsx, pairing.tsx: "st warn live big-state"
  "sync-macts": "acts", // sync.tsx: "acts sync-macts"
  "sync-kit": "confirm", // sync.tsx: "confirm sync-kit"
  "sheet-back": "flink", // sign-in.tsx: "flink sheet-back"
  urgent: "defaults-card", // home.tsx: "defaults-card account-card", urgent when on top
};

const isFlex = (display?: string) => display === "flex" || display === "inline-flex";

/** The selector with `.no-flex-gap` on <html>: merged into a first compound that is the root's own, else before it. */
function underNoGap(sel: Sel): Sel {
  const first = sel.findIndex(isComb);
  const head = first < 0 ? sel : sel.slice(0, first);
  const onRoot = head.some((c) => (c.type === "attribute" && ["data-form", "data-narrow"].includes((c as { name: string }).name)) || pseudo(c, "root"));
  const mark = { type: "class", name: "no-flex-gap" } as SelectorComponent;
  return onRoot ? [...head, mark, ...sel.slice(head.length)] : [mark, { type: "combinator", value: "descendant" } as SelectorComponent, ...sel];
}

const child: Sel = [{ type: "combinator", value: "child" }, { type: "universal" }] as Sel;
const next: Sel = [{ type: "combinator", value: "next-sibling" }, { type: "universal" }] as Sel;

type Margin = { tail: Sel; prop: string; value: string };
const svgOnly: Sel = [{ type: "combinator", value: "child" }, { type: "type", name: "svg" }, { type: "pseudo-class", kind: "only-child" }] as Sel;
const zero = (v?: string) => !v || v === "0" || v === "0px" || v === "normal";

/** The child margins that stand in for a flex box's gap: after the first child along a single line, at each child's
 * end when the box wraps, and on an icon that sits before a bare text (67 buttons do it, none is an icon alone: a text
 * node takes no margin, so the icon does). */
function margins(flex: Flex): Margin[] {
  if (!isFlex(flex.display)) return [];
  const direction = flex.direction ?? "row";
  const column = direction.startsWith("column");
  const along = column ? flex.gap?.row : flex.gap?.column;
  if (zero(along)) return [];
  const side = { row: "left", "row-reverse": "right", column: "top", "column-reverse": "bottom" }[direction] ?? "left";
  const end = { left: "right", right: "left", top: "bottom", bottom: "top" }[side];
  const wraps = !!flex.wrap && flex.wrap !== "nowrap";
  // ponytail: a wrapping box keeps its gap along each line as an end margin on every child (a line may break one gap
  // early) and loses it between lines; negative margins on the box would fight the box's own.
  return [wraps ? { tail: child, prop: `margin-${end}`, value: along! } : { tail: [...child, ...next], prop: `margin-${side}`, value: along! }, { tail: svgOnly, prop: `margin-${end}`, value: along! }];
}

/** The rules that stand in for a flex box's `gap` under `.no-flex-gap`. A box that takes its display from a class
 * (`.info .acts .btn` is a `.btn`) also gets that class's margins set back to 0 where its own differ. */
function gapRules(rule: StyleRule, texts: string[], facts: Map<string, Flex>, loose: string[]): Rule[] {
  const own = flexFacts(texts);
  if (!own.gap || (zero(own.gap.row) && zero(own.gap.column))) return [];
  // A multi-column box's column gap is old enough.
  if (texts.some((t) => /^column(s|-count|-width):/.test(t))) return [];
  const lookup = (keysOf: (sel: Sel, printed: string, classes: string[]) => string[]) =>
    rule.selectors.map((sel) => {
      const printed = printSel(sel);
      const classes = sel.slice(afterLastComb(sel)).flatMap((c) => (c.type === "class" ? [(c as { name: string }).name] : []));
      return keysOf(sel, printed, classes).map((k) => facts.get(k)).find((f) => f?.display);
    });
  const byClass = (n: string) => [n, ...(SITS_WITH[n] ? [SITS_WITH[n]] : [])];
  // Display from the rule, else from the whole selector or a class of its last compound (or the class it sits beside).
  const found = own.display
    ? [own]
    : lookup((_, printed, classes) => [bySelector(printed), ...classes.flatMap((n) => [...byClass(n), ...(SITS_WITH[n] ? [bySelector(printed.replace(`.${n}`, `.${SITS_WITH[n]}`))] : [])])]);
  const first = found[0];
  if (!first || found.some((f) => f?.display !== first.display)) {
    loose.push(rule.selectors.map(printSel).join(", "));
    return [];
  }
  const flex: Flex = { ...first, ...Object.fromEntries(Object.entries(own).filter(([, v]) => v)), gap: own.gap };
  const mine = margins(flex);
  // The class's own margins reach this box's children too: each one this box does not set itself goes back to 0.
  const base = lookup((sel, _, classes) => (sel.length === 1 ? [] : classes.flatMap(byClass)))[0];
  const theirs = base ? margins(base) : [];
  const same = (a: Margin, b: Margin) => a.prop === b.prop && JSON.stringify(a.tail) === JSON.stringify(b.tail);
  const all = [...mine, ...theirs.filter((t) => !mine.some((m) => same(m, t))).map((t) => ({ ...t, value: "0" }))];
  const selectors = rule.selectors.filter((sel) => !sel.some((c) => c.type === "pseudo-element")).map(underNoGap);
  if (!selectors.length) return [];
  return all.map(
    (m) => ({ type: "style", value: { selectors: selectors.map((s) => [...s, ...m.tail]), declarations: { declarations: parseDecls(`${m.prop}: ${m.value}`), importantDeclarations: [] }, loc: rule.loc } }) as Rule,
  );
}

/** A child pushed to the end with `margin-left: auto` keeps it under `.no-flex-gap`, over its box's gap margins. */
function autoRules(rule: StyleRule, texts: string[]): Rule[] {
  const auto = texts.flatMap((t) => {
    const [prop, value = ""] = t.split(/:\s*/, 2);
    if (prop === "margin-left" || prop === "margin-top") return value === "auto" ? [`${prop}: auto`] : [];
    if (prop !== "margin") return [];
    const [top, right = top, , left = right] = words(value);
    return [...(top === "auto" ? ["margin-top: auto"] : []), ...(left === "auto" ? ["margin-left: auto"] : [])];
  });
  const selectors = rule.selectors.filter((sel) => !sel.some((c) => c.type === "pseudo-element")).map(underNoGap);
  if (!auto.length || !selectors.length) return [];
  return [{ type: "style", value: { selectors, declarations: { declarations: parseDecls(auto.join(";")), importantDeclarations: [] }, loc: rule.loc } } as Rule];
}

// ---- The visitor --------------------------------------------------------------------------------------------------

/** The CSS visitor of the webOS build. It runs on each module (Vite's per-file pass) and not again on the minifier's
 * pass over the joined bundle, which only sees `style.css` (Vite's name for it) and has nothing left to do. */
export function chromium79Visitor() {
  const tokens = rootTokens();
  const facts = classFacts();
  let active = true;
  const lost: string[] = [];
  const loose: string[] = [];
  const unstood: string[] = [];
  return {
    StyleSheet(sheet: { sources: string[] }) {
      active = sheet.sources.some((s) => s.endsWith(".css") && s.includes("/"));
      lost.length = 0;
      loose.length = 0;
      unstood.length = 0;
    },
    StyleSheetExit(sheet: { sources: string[] }) {
      if (!active) return;
      const unknown = lost.filter((s) => !(s in KNOWN_LOSSES));
      if (unknown.length) throw new Error(`${sheet.sources.join(", ")}: Chromium 79 cannot read these TV selectors:\n  ${unknown.join("\n  ")}\nRewrite them, or list them in KNOWN_LOSSES (scripts/webos-build.ts) with what stands in.`);
      if (loose.length) throw new Error(`${sheet.sources.join(", ")}: these set a gap and no display Chromium 79 can be told about:\n  ${loose.join("\n  ")}\nAdd the class they sit beside to SITS_WITH (scripts/webos-build.ts).`);
      if (unstood.length) throw new Error(`${sheet.sources.join(", ")}: aspect-ratio (Chromium 79 reads none) with nothing in its place:\n  ${unstood.join("\n  ")}\nStand in for it in theme/webos.css and list it in ASPECT_RATIO_STANDINS (scripts/webos-build.ts).`);
    },
    Rule: {
      style(rule: { type: "style"; value: StyleRule }): Rule[] | void {
        if (!active) return;
        const value = rule.value;
        const kept: Sel[] = [];
        let changed = false;
        for (const sel of value.selectors) {
          // A phone or desktop rule goes before it is written out: its `:is()` never has to keep its weight here.
          if (otherForm(sel)) {
            changed = true;
            continue;
          }
          const written = expand(sel);
          if (!written) {
            lost.push(printSel(sel));
            changed = true;
            continue;
          }
          for (const one of written.map(lower)) {
            if (otherForm(one)) {
              changed = true;
              continue;
            }
            const why = unreadable(one);
            if (why) {
              lost.push(printSel(one));
              changed = true;
              continue;
            }
            kept.push(one);
          }
        }
        // The same selector twice (from two `:is()` written out) is one.
        const seen = new Set<string>();
        const unique = kept.filter((s) => {
          const key = JSON.stringify(s);
          if (seen.has(key)) return false;
          seen.add(key);
          return true;
        });
        if (!unique.length) return [];
        if (JSON.stringify(unique) !== JSON.stringify(value.selectors)) changed = true;
        value.selectors = unique;
        const block = value.declarations as Decls | undefined;
        const normal = block?.declarations ?? [];
        const important = block?.importantDeclarations ?? [];
        if ([...normal, ...important].some(isAspectRatio)) unstood.push(...unique.map(printSel).filter((s) => !(s in ASPECT_RATIO_STANDINS)));
        const lowNormal = lowerDecls(normal, tokens);
        const lowImportant = lowerDecls(important, tokens);
        if (lowNormal || lowImportant) {
          value.declarations = { declarations: lowNormal ?? normal, importantDeclarations: lowImportant ?? important };
          changed = true;
        }
        const texts = (value.declarations?.declarations ?? []).map(printDecl);
        const extra = [...gapRules(value, texts, facts, loose), ...autoRules(value, texts)];
        if (!changed && !extra.length) return;
        return clean([rule as Rule, ...extra]);
      },
    },
  };
}
