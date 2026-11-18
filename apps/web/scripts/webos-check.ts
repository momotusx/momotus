/** Checks the webOS bundle (`vite build --mode webos`, dist-webos/) against Chromium 79 (plans/webos.md section 2),
 * then plants one mutation per rule into a copy and checks that each one fails it.
 *   bun scripts/webos-check.ts [dist dir]
 * Exit 0 only when the bundle passes and every mutation is caught. */

import { readdirSync, readFileSync } from "node:fs";
import { join } from "node:path";
import { parseAst } from "vite";

type CssRule = { selector: string; body: string };

/** The style rules of a minified sheet, at any depth of @media or @supports; keyframes and font faces are skipped. */
export function cssRules(css: string): CssRule[] {
  const out: CssRule[] = [];
  const walk = (text: string) => {
    let i = 0;
    while (i < text.length) {
      const open = text.indexOf("{", i);
      if (open < 0) return;
      const prelude = text.slice(i, open).trim();
      let depth = 1;
      let j = open + 1;
      let quote = "";
      for (; j < text.length && depth > 0; j++) {
        const ch = text[j];
        if (quote) {
          if (ch === "\\") j++;
          else if (ch === quote) quote = "";
        } else if (ch === '"' || ch === "'") quote = ch;
        else if (ch === "{") depth++;
        else if (ch === "}") depth--;
      }
      const body = text.slice(open + 1, j - 1);
      if (prelude.startsWith("@media") || prelude.startsWith("@supports")) walk(body);
      else if (!prelude.startsWith("@")) out.push({ selector: prelude, body });
      i = j;
    }
  };
  walk(css.replace(/\/\*[\s\S]*?\*\//g, ""));
  return out;
}

/** Splits a selector list at its top-level commas. */
const selectors = (list: string) => list.split(/,(?![^()]*\))/).map((s) => s.trim());

/** The TV form never matches it: its first compound asks for the phone or desktop form. */
const otherForm = (sel: string) => /^[^\s>+~]*\[data-form=["']?(phone|desktop)["']?\]/.test(sel);

/** A `:not()` whose argument is not one simple selector (Chrome 88 reads the rest). */
function complexNot(sel: string): boolean {
  for (const m of sel.matchAll(/:not\(/g)) {
    let depth = 1;
    let k = m.index! + 5;
    const start = k;
    for (; k < sel.length && depth > 0; k++) {
      if (sel[k] === "(") depth++;
      if (sel[k] === ")") depth--;
    }
    const arg = sel.slice(start, k - 1);
    if (!/^([.#]?-?[\w-]+|\[[^\]]+\]|:[\w-]+(\([^()]*\))?|\*)$/.test(arg)) return true;
  }
  return false;
}

const SELECTOR_BANS: [RegExp, string][] = [
  [/:has\(/, ":has("],
  [/:is\(/, ":is("],
  [/:where\(/, ":where("],
  [/:focus-visible/, ":focus-visible"],
  [/:dir\(/, ":dir("],
];
const BODY_BANS: [RegExp, string][] = [
  [/subgrid/, "subgrid"],
  [/\d[dsl]vh\b/, "dvh, svh or lvh"],
  [/color-mix\(/, "color-mix("],
  [/(^|;)translate:/, "translate:"],
  [/(^|;)(padding|margin)-inline:/, "padding-inline: or margin-inline:"],
  [/(^|;)overflow(-x|-y)?:[^;]*\bclip\b/, "overflow: clip"],
];

const flexRule = (body: string) => /(^|;)display:(inline-)?flex(;|$)/.test(body);
const gapSet = (body: string) => [...body.matchAll(/(?:^|;)(?:row-|column-)?gap:([^;]*)/g)].some((m) => !/^(0|0px|normal)( (0|0px|normal))?$/.test(m[1]!.trim()));
/** A `.no-flex-gap` rule's selector as the rule it stands in for. */
const pairedFrom = (sel: string) =>
  sel
    .replace(/^\.no-flex-gap /, "")
    .replace(/\.no-flex-gap/, "")
    .replace(/>\*(\+\*)?$/, "");

/** Every way the CSS fails Chromium 79, empty when it passes. */
export function checkCss(css: string): string[] {
  const problems: string[] = [];
  const rules = cssRules(css);
  const paired = new Set(rules.flatMap((r) => selectors(r.selector).filter((s) => s.includes("no-flex-gap")).map(pairedFrom)));
  for (const r of rules) {
    const list = selectors(r.selector);
    if (list.every(otherForm)) continue;
    for (const [re, name] of SELECTOR_BANS) if (re.test(r.selector)) problems.push(`${name} in ${r.selector}`);
    if (list.some(complexNot)) problems.push(`complex :not( in ${r.selector}`);
    for (const [re, name] of BODY_BANS) if (re.test(r.body)) problems.push(`${name} in ${r.selector}{${r.body}}`);
    if (flexRule(r.body) && gapSet(r.body) && !r.selector.includes("no-flex-gap")) {
      for (const s of list) if (!otherForm(s) && !/::?(before|after)\b/.test(s) && !paired.has(s)) problems.push(`flex gap with no .no-flex-gap margins: ${s}`);
    }
  }
  return problems;
}

/** The page sets `no-flex-gap` before its first paint. */
export const checkHtml = (html: string) => (html.includes("no-flex-gap") ? [] : ["index.html has no flex gap probe"]);

/** APIs newer than Chromium 79 that the app has no fallback for. */
const JS_BANS: [RegExp, string][] = [
  [/\bstructuredClone\(/, "structuredClone"],
  [/\.findLast(Index)?\(/, "findLast"],
  [/\.to(Sorted|Reversed|Spliced)\(/, "toSorted/toReversed/toSpliced"],
  [/\b(Object|Map)\.groupBy\b/, "groupBy"],
  [/\bPromise\.(withResolvers|any)\b/, "Promise.withResolvers/any"],
  [/\bAbortSignal\.any\b/, "AbortSignal.any"],
  [/\bnew WeakRef\(|\bFinalizationRegistry\b/, "WeakRef/FinalizationRegistry"],
  [/\bArray\.fromAsync\b/, "Array.fromAsync"],
  [/\bhls\.js\b|\bHls\.Events\b|MANIFEST_PARSED/, "hls.js (never loaded on webOS)"],
];
/** Every string a script holds: its string literals and the parts of its template literals. */
function scriptStrings(code: string): string[] {
  const out: string[] = [];
  const walk = (node: unknown): void => {
    if (!node || typeof node !== "object") return;
    if (Array.isArray(node)) return node.forEach(walk);
    const n = node as { type?: string; value?: unknown };
    if (n.type === "Literal" && typeof n.value === "string") out.push(n.value);
    if (n.type === "TemplateElement") out.push((n.value as { raw: string }).raw);
    Object.values(node).forEach(walk);
  };
  walk(parseAst(code));
  return out;
}

/** What Chromium 79 cannot parse in a selector a script hands to querySelector, matches or closest: it throws there. */
const scriptSelectorBan = (s: string) => /:(focus-visible|has\(|is\(|where\()/.exec(s)?.[0] ?? (complexNot(s) ? "a complex :not(" : null);

/** APIs newer than Chromium 79 the app uses, each installed by lib/compat.ts: the installer's name must be there. */
const COMPAT = ["replaceAll", "at", "hasOwn", "timeout", "randomUUID", "replaceChildren", "DisplayNames", "Segmenter", "onscrollend", "data-mq-inert-hid"];

/** Every way the scripts fail Chromium 79, empty when they pass. */
export function checkJs(files: Record<string, string>): string[] {
  const problems: string[] = [];
  const all = Object.values(files).join("\n");
  for (const [name, code] of Object.entries(files)) for (const [re, what] of JS_BANS) if (re.test(code)) problems.push(`${what} in ${name}`);
  for (const [name, code] of Object.entries(files))
    for (const s of scriptStrings(code)) {
      const what = scriptSelectorBan(s);
      if (what) problems.push(`${what} in a script selector, "${s}" in ${name}`);
    }
  for (const name of COMPAT) if (!all.includes(`"${name}"`)) problems.push(`lib/compat.ts no longer installs ${name}`);
  if (Object.keys(files).some((f) => /(^|\/)hls[-.]/.test(f))) problems.push("an hls.js chunk is in the bundle");
  return problems;
}

/** One planted change each; `fails: false` is a change the check must let through. */
const CSS_MUTATIONS: { name: string; add: string; fails: boolean }[] = [
  { name: "an unscoped :has(", add: ".mq-m:has(.mq-n){color:red}", fails: true },
  { name: "a phone-scoped :has(", add: "[data-form=phone] .mq-m:has(.mq-n){color:red}", fails: false },
  { name: ":is(", add: ":is(.mq-a,.mq-b){color:red}", fails: true },
  { name: ":where(", add: ":where(.mq-a){color:red}", fails: true },
  { name: ":focus-visible", add: ".mq-a:focus-visible{color:red}", fails: true },
  { name: "a complex :not(", add: ".mq-a:not(.mq-b .mq-c){color:red}", fails: true },
  { name: "subgrid", add: ".mq-a{grid-template-columns:subgrid}", fails: true },
  { name: "dvh", add: ".mq-a{height:100dvh}", fails: true },
  { name: "svh", add: ".mq-a{height:100svh}", fails: true },
  { name: "color-mix( after a fallback", add: ".mq-a{background:#fff;background:color-mix(in srgb,var(--text) 5%,transparent)}", fails: true },
  { name: "a flex gap with no fallback", add: ".mq-a{gap:4px;display:flex}", fails: true },
  { name: "overflow: clip", add: ".mq-a{overflow:hidden clip}", fails: true },
  { name: "translate:", add: ".mq-a{translate:0 -50%}", fails: true },
  { name: "padding-inline:", add: ".mq-a{padding-inline:4px}", fails: true },
  { name: "margin-inline:", add: ".mq-a{margin-inline:4px}", fails: true },
];
const plant = (code: string) => (f: Record<string, string>) => ({ ...f, "planted.js": code });
const JS_MUTATIONS: { name: string; change: (f: Record<string, string>) => Record<string, string>; fails: boolean }[] = [
  { name: "structuredClone", change: plant("const a=structuredClone(b);"), fails: true },
  { name: "a querySelector(':focus-visible')", change: plant('document.querySelector(".a:focus-visible")'), fails: true },
  { name: "a querySelector(':is(')", change: plant('document.querySelector("main :is(.a, .b) [data-x]")'), fails: true },
  { name: "a matches(':where(')", change: plant('a.matches(":where(.a) .b")'), fails: true },
  { name: "a complex :not( in a template", change: plant('document.querySelector(`[data-f="${a}"]:not([inert] *)`)'), fails: true },
  { name: "a :not( of one simple selector", change: plant('document.querySelector("[data-f]:not([inert])")'), fails: false },
  { name: "is() called after a minified key", change: plant("const o={index:is().filter(Boolean)}"), fails: false },
  { name: "hls.js loaded", change: plant('import("./hls-x.js").then(m=>m.default.Events.MANIFEST_PARSED)'), fails: true },
  {
    fails: true,
    name: "a compat fallback removed",
    change: (f) => Object.fromEntries(Object.entries(f).map(([k, v]) => [k, v.replaceAll('"replaceAll"', '"replaceAllGone"')])),
  },
];

function main() {
  const dist = process.argv[2] ?? join(import.meta.dirname, "../dist-webos");
  const assets = join(dist, "assets");
  const names = readdirSync(assets);
  const css = names.filter((n) => n.endsWith(".css")).map((n) => readFileSync(join(assets, n), "utf8")).join("\n");
  const js = Object.fromEntries(names.filter((n) => n.endsWith(".js")).map((n) => [n, readFileSync(join(assets, n), "utf8")]));
  const html = readFileSync(join(dist, "index.html"), "utf8");
  // A mutation counts as caught when it adds a problem to the bundle's own.
  const base = { css: checkCss(css).length, html: checkHtml(html).length, js: checkJs(js).length };
  const problems = [...checkCss(css), ...checkHtml(html), ...checkJs(js)];
  for (const p of problems) console.error(`FAIL ${p}`);
  const rules = cssRules(css);
  console.log(`css: ${rules.length} rules, ${rules.filter((r) => r.selector.includes("no-flex-gap")).length} .no-flex-gap rules; js: ${Object.keys(js).length} files`);
  let missed = 0;
  for (const m of CSS_MUTATIONS) {
    const failed = checkCss(`${css}\n${m.add}`).length > base.css;
    const ok = failed === m.fails;
    if (!ok) missed++;
    console.log(`${ok ? "ok  " : "MISS"} css mutation, ${m.name}: ${failed ? "fails" : "passes"}`);
  }
  {
    const failed = checkHtml(html.replaceAll("no-flex-gap", "x")).length > base.html;
    if (!failed) missed++;
    console.log(`${failed ? "ok  " : "MISS"} html mutation, no flex gap probe: ${failed ? "fails" : "passes"}`);
  }
  for (const m of JS_MUTATIONS) {
    const failed = checkJs(m.change(js)).length > base.js;
    const ok = failed === m.fails;
    if (!ok) missed++;
    console.log(`${ok ? "ok  " : "MISS"} js mutation, ${m.name}: ${failed ? "fails" : "passes"}`);
  }
  console.log(problems.length || missed ? `webos check FAILED: ${problems.length} problems, ${missed} mutations missed` : "webos check passed, every mutation caught");
  process.exit(problems.length || missed ? 1 : 0);
}

if (import.meta.main) main();
