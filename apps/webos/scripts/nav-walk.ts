// The shared navigation walk (plans/perf-nav.md) over raw CDP, for engines Playwright does not drive: a Chromium 79 or
// 92 snapshot running the webOS bundle, or a real TV's inspector (ares-inspect --open gives its address). Per press: the
// time from the key to the end of the first frame showing its change (focus, page or field), the longest frame gap
// until the next key, and from a trace the longest main-thread task and its split. Rows go to --out as JSONL.
//
//   bun apps/webos/scripts/nav-walk.ts --chrome <Chromium binary> --url 'http://127.0.0.1:47801/#t=<token>' --rate 4
//   bun apps/webos/scripts/nav-walk.ts --attach http://<tv>:9998 --rate 1
//
// --chrome launches it headless at 1920x1080 with a webOS user agent and a profile under $TMPDIR; --attach takes the
// first page of a running inspector and reloads nothing (start on Home with the hero's Play focused). --rate is CDP CPU
// throttling (none on a TV), --reps repeats the walk (reloading --url each time), --label names the rows.

import { spawn, type ChildProcess } from "node:child_process";
import { appendFileSync, mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { parseArgs } from "node:util";

const { values: opt } = parseArgs({
  options: {
    chrome: { type: "string" },
    attach: { type: "string" },
    url: { type: "string" },
    rate: { type: "string", default: "1" },
    reps: { type: "string", default: "1" },
    label: { type: "string", default: "walk" },
    out: { type: "string", default: "nav-walk.jsonl" },
  },
});
if (!opt.chrome === !opt.attach) throw new Error("give --chrome or --attach");
if (opt.chrome && !opt.url) throw new Error("--chrome needs --url");

const UA = "Mozilla/5.0 (Web0S; Linux/SmartTV) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/79.0.3945.79 Safari/537.36 WebAppManager";
const sleep = (ms: number) => new Promise((r) => setTimeout(r, ms));

type Ev = { method: string; params: any };
class Cdp {
  private id = 0;
  private wait = new Map<number, (m: any) => void>();
  on: ((e: Ev) => void)[] = [];
  private constructor(private ws: WebSocket) {}
  static async open(url: string) {
    const c = new Cdp(new WebSocket(url));
    c.ws.onmessage = (m) => {
      const d = JSON.parse(String(m.data));
      if (d.id === undefined) for (const f of c.on) f(d);
      else c.wait.get(d.id)?.(d), c.wait.delete(d.id);
    };
    await new Promise((r) => (c.ws.onopen = r));
    return c;
  }
  send(method: string, params: object = {}): Promise<any> {
    const id = ++this.id;
    this.ws.send(JSON.stringify({ id, method, params }));
    return new Promise((ok, fail) => this.wait.set(id, (d) => (d.error ? fail(new Error(`${method}: ${JSON.stringify(d.error)}`)) : ok(d.result))));
  }
  async eval<T>(expression: string): Promise<T> {
    const r = await this.send("Runtime.evaluate", { expression, returnByValue: true, awaitPromise: true });
    if (r.exceptionDetails) throw new Error(JSON.stringify(r.exceptionDetails).slice(0, 300));
    return r.result.value;
  }
  next(method: string, ms: number): Promise<any> {
    return new Promise((ok, fail) => {
      const t = setTimeout(() => fail(new Error(`no ${method}`)), ms);
      const f = (e: Ev) => {
        if (e.method !== method) return;
        clearTimeout(t);
        this.on = this.on.filter((x) => x !== f);
        ok(e.params);
      };
      this.on.push(f);
    });
  }
  close() {
    this.ws.close();
  }
}

async function pageOf(base: string) {
  for (let i = 0; i < 300; i++) {
    try {
      const list: { type: string; webSocketDebuggerUrl: string }[] = await (await fetch(`${base}/json/list`)).json();
      const page = list.find((t) => t.type === "page");
      if (page) return page.webSocketDebuggerUrl;
    } catch {}
    await sleep(200);
  }
  throw new Error(`no page at ${base}`);
}

// D-pad keys as a remote sends them: Back is webOS's 461.
const KEYS: Record<string, [number, string]> = {
  ArrowDown: [40, "ArrowDown"], ArrowUp: [38, "ArrowUp"], ArrowLeft: [37, "ArrowLeft"], ArrowRight: [39, "ArrowRight"],
  Enter: [13, "Enter"], Back: [461, "GoBack"],
};
async function press(c: Cdp, k: string) {
  if (k.startsWith("t:")) {
    const ch = k.slice(2);
    const code = ch.toUpperCase().charCodeAt(0);
    await c.send("Input.dispatchKeyEvent", { type: "keyDown", key: ch, code: `Key${ch.toUpperCase()}`, text: ch, unmodifiedText: ch, windowsVirtualKeyCode: code });
    return c.send("Input.dispatchKeyEvent", { type: "keyUp", key: ch, code: `Key${ch.toUpperCase()}`, windowsVirtualKeyCode: code });
  }
  const [code, key] = KEYS[k] as [number, string];
  const base = { key, code: key, windowsVirtualKeyCode: code, nativeVirtualKeyCode: code };
  await c.send("Input.dispatchKeyEvent", { ...(k === "Enter" ? { type: "keyDown", text: "\r", unmodifiedText: "\r" } : { type: "rawKeyDown" }), ...base });
  await c.send("Input.dispatchKeyEvent", { type: "keyUp", ...base });
}

const SLOW = 2000;
const WALK: [action: string, key: string, wait?: number][] = [
  ["home down", "ArrowDown"], ["home down", "ArrowDown"], ["home down", "ArrowDown"],
  ["home right", "ArrowRight"], ["home right", "ArrowRight"], ["home right", "ArrowRight"], ["home right", "ArrowRight"], ["home right", "ArrowRight"],
  ["open title", "Enter", SLOW], ["title right", "ArrowRight"], ["open versions", "Enter", SLOW],
  ["back to title", "Back", SLOW], ["back to home", "Back", SLOW],
  ["home up", "ArrowUp"], ["home up", "ArrowUp"], ["home up", "ArrowUp"], ["home up", "ArrowUp"],
  ["bar to discover", "ArrowRight", SLOW], ["discover down", "ArrowDown"], ["discover down", "ArrowDown"],
  ["discover right", "ArrowRight"], ["discover right", "ArrowRight"], ["discover right", "ArrowRight"],
  ["discover up", "ArrowUp"], ["discover up", "ArrowUp"], ["discover up", "ArrowUp"],
  ["bar right", "ArrowRight", SLOW], ["bar right", "ArrowRight", SLOW], ["bar to search", "ArrowRight", SLOW],
  ["search down", "ArrowDown"], ["type", "t:b", 900], ["type", "t:a", 900], ["type", "t:t", 1500], ["search back", "Back", SLOW],
];

// Per key: its time, the first frame's end, the end of the first frame whose animation frame saw a new focus, path or
// field value (until a newer key), and every frame's start for 1.5 s.
const PROBE = `(() => {
  if (window.__walk) return;
  window.__walk = [];
  const state = () => { const a = document.activeElement; return (a && a.getAttribute('data-f')) + '|' + location.pathname + '|' + (a && 'value' in a ? a.value : ''); };
  addEventListener('keydown', (e) => {
    const rec = { t0: Math.min(e.timeStamp > 1e5 || e.timeStamp <= 0 ? performance.now() : e.timeStamp, performance.now()), before: state(), frames: [] };
    window.__walk.push(rec);
    const after = (f) => { const ch = new MessageChannel(); ch.port1.onmessage = () => f(performance.now()); ch.port2.postMessage(0); };
    const loop = (t) => {
      rec.frames.push(t);
      if (rec.next === undefined) { rec.next = -1; after((n) => (rec.next = n)); }
      if (rec.change === undefined && window.__walk[window.__walk.length - 1] === rec && state() !== rec.before) { rec.change = -1; after((n) => (rec.change = n)); }
      if (t - rec.t0 < 1500) requestAnimationFrame(loop);
    };
    requestAnimationFrame(loop);
  }, true);
})()`;

type T = { name: string; ph: string; ts: number; dur?: number; pid: number; tid: number; args?: any };

// Old engines write some events as B/E pairs; per key, the renderer main thread's longest task, and the split of the
// time inside that window's tasks (each event's own time, under its nearest named ancestor).
const SPLIT: Record<string, string> = {
  FunctionCall: "script", EventDispatch: "script", TimerFire: "script", FireAnimationFrame: "script", EvaluateScript: "script",
  UpdateLayoutTree: "style", Layout: "layout", Paint: "paint", PrePaint: "paint", UpdateLayerTree: "paint", Layerize: "paint", CompositeLayers: "paint",
};
function perKey(raw: T[], keys: number[]) {
  const open = new Map<string, T[]>();
  const xs: T[] = [];
  for (const e of [...raw].sort((a, b) => a.ts - b.ts)) {
    const k = `${e.pid}:${e.tid}`;
    if (e.ph === "B") (open.get(k) ?? open.set(k, []).get(k)!).push(e);
    else if (e.ph === "E") {
      const b = open.get(k)?.pop();
      if (b) xs.push({ ...b, ph: "X", dur: e.ts - b.ts });
    } else if (e.ph === "X") xs.push(e);
  }
  const names = new Map<string, string>();
  for (const e of raw) if (e.ph === "M" && e.name === "thread_name") names.set(`${e.pid}:${e.tid}`, e.args?.name);
  const count = new Map<string, number>();
  for (const e of xs) count.set(`${e.pid}:${e.tid}`, (count.get(`${e.pid}:${e.tid}`) ?? 0) + 1);
  const main = [...names].filter(([, n]) => n === "CrRendererMain").map(([k]) => k).sort((a, b) => (count.get(b) ?? 0) - (count.get(a) ?? 0))[0];
  const own = xs.filter((e) => `${e.pid}:${e.tid}` === main).sort((a, b) => a.ts - b.ts || (b.dur ?? 0) - (a.dur ?? 0));
  const seg = (ts: number) => keys.findIndex((k, i) => ts >= k && ts < (keys[i + 1] ?? Infinity));
  const rows = keys.map(() => ({ longestTaskMs: 0, script: 0, style: 0, layout: 0, paint: 0 }));
  const stack: { e: T; cat?: string; child: number }[] = [];
  const done = (top: { e: T; cat?: string; child: number }) => {
    const s = seg(top.e.ts);
    if (s >= 0 && top.cat) (rows[s] as any)[top.cat] += ((top.e.dur ?? 0) - top.child) / 1000;
  };
  for (const e of own) {
    while (stack.length && stack[stack.length - 1]!.e.ts + (stack[stack.length - 1]!.e.dur ?? 0) <= e.ts) done(stack.pop()!);
    const parent = stack[stack.length - 1];
    if (parent) parent.child += e.dur ?? 0;
    else if (/RunTask$/.test(e.name)) {
      const s = seg(e.ts);
      if (s >= 0) rows[s]!.longestTaskMs = Math.max(rows[s]!.longestTaskMs, (e.dur ?? 0) / 1000);
    }
    stack.push({ e, cat: SPLIT[e.name] ?? parent?.cat, child: 0 });
  }
  while (stack.length) done(stack.pop()!);
  return rows.map((r) => Object.fromEntries(Object.entries(r).map(([k, v]) => [k, Math.round(v * 10) / 10])));
}

const rate = Number(opt.rate);
let proc: ChildProcess | undefined;
let profile: string | undefined;
let base = opt.attach;
if (opt.chrome) {
  profile = mkdtempSync(join(tmpdir(), "nav-walk-"));
  const port = 9300 + Math.floor(Math.random() * 500);
  proc = spawn(opt.chrome, [
    "--headless", `--remote-debugging-port=${port}`, `--user-data-dir=${profile}`, "--mute-audio", "--no-first-run",
    "--no-default-browser-check", "--window-size=1920,1080", "--force-device-scale-factor=1", `--user-agent=${UA}`, "about:blank",
  ], { stdio: "ignore" });
  base = `http://127.0.0.1:${port}`;
}
const c = await Cdp.open(await pageOf(base as string));
try {
  await c.send("Page.enable");
  await c.send("Runtime.enable");
  if (opt.chrome) await c.send("Emulation.setDeviceMetricsOverride", { width: 1920, height: 1080, deviceScaleFactor: 1, mobile: false });
  const focused = () => c.eval<string>("(document.activeElement && document.activeElement.getAttribute('data-f')) || ''").catch(() => "");
  for (let rep = 0; rep < Number(opt.reps); rep++) {
    if (opt.url) await c.send("Page.navigate", { url: opt.url });
    for (let i = 0; i < 120 && (await focused()) !== "hero:play"; i++) await sleep(500);
    if ((await focused()) !== "hero:play") throw new Error("Home's Play never took the focus");
    await sleep(5000);
    await c.eval(PROBE);
    if (rate !== 1) await c.send("Emulation.setCPUThrottlingRate", { rate });
    const events: T[] = [];
    const collect = (e: Ev) => e.method === "Tracing.dataCollected" && events.push(...e.params.value);
    c.on.push(collect);
    await c.send("Tracing.start", {
      traceConfig: { recordMode: "recordAsMuchAsPossible", includedCategories: ["devtools.timeline", "disabled-by-default-devtools.timeline", "toplevel", "__metadata"] },
      transferMode: "ReportEvents",
    });
    const seen: string[] = [];
    for (const [, key, wait] of WALK) {
      await press(c, key);
      await sleep(wait ?? 600);
      seen.push(await focused());
    }
    const done = c.next("Tracing.tracingComplete", 300_000);
    await c.send("Tracing.end");
    await done;
    c.on = c.on.filter((f) => f !== collect);
    if (rate !== 1) await c.send("Emulation.setCPUThrottlingRate", { rate: 1 });
    const page: any[] = await c.eval("window.__walk.splice(0)");
    const keys = events.filter((e) => e.name === "EventDispatch" && e.args?.data?.type === "keydown").map((e) => e.ts).sort((a, b) => a - b);
    const split = keys.length === WALK.length ? perKey(events, keys) : [];
    WALK.forEach(([action, key], i) => {
      const p = page[i] ?? {};
      const fr: number[] = p.frames ?? [];
      const gaps = fr.slice(1).map((t, j) => t - (fr[j] as number));
      const ms = (t: number) => (t > 0 ? Math.round((t - p.t0) * 10) / 10 : null);
      appendFileSync(opt.out as string, `${JSON.stringify({
        date: new Date().toISOString().slice(0, 10), label: opt.label, cpuThrottle: rate, rep, i, action, key, focus: seen[i],
        nextFrameMs: ms(p.next), changeFrameMs: ms(p.change), frameGapMaxMs: Math.round(Math.max(0, ...gaps) * 10) / 10,
        framesOver33: gaps.filter((g) => g > 33.4).length, ...(split[i] ?? {}),
      })}\n`);
    });
    console.log(`${opt.label} x${rate} rep ${rep}: ${keys.length}/${WALK.length} keys traced, ended on ${seen[seen.length - 1]}`);
  }
} finally {
  c.close();
  proc?.kill();
  if (profile) rmSync(profile, { recursive: true, force: true });
}
