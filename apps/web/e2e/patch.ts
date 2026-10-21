/** A browser context whose core messages pass through a test's `patch` on
 * their way to the page, which watches what the core sent or writes what the
 * e2e bridge cannot reach (it has no torrent engine): a torrent's start for
 * the loading layer, a version that plays from this device. Every other
 * message passes as the core sent it. */

import type { Browser, BrowserContextOptions, WebSocketRoute } from "@playwright/test";
import { PORTS } from "./core";

// Snapshots are whatever the core sent.
type Json = any;

export async function patchedContext(browser: Browser, port: number, options: BrowserContextOptions, patch: (view: string, snapshot: Json) => Json) {
  const context = await browser.newContext({ colorScheme: "dark", baseURL: `http://127.0.0.1:${PORTS.vite}`, ...options });
  // Subscribe requests by id, then live views by sub, each with the last snapshot the core sent.
  const asked = new Map<number, string>();
  const subs = new Map<number, { view: string; last: Json }>();
  let page: WebSocketRoute | null = null;
  await context.routeWebSocket(/\/ws$/, (ws) => {
    page = ws;
    const upstream = new WebSocket(`ws://127.0.0.1:${port}/ws`);
    const early: string[] = [];
    upstream.addEventListener("open", () => {
      for (const m of early.splice(0)) upstream.send(m);
    });
    upstream.addEventListener("message", (e) => {
      const m = JSON.parse(String(e.data));
      const view = asked.get(m.id);
      if (view && m.ok && m.result && typeof m.result === "object" && "snapshot" in m.result) {
        subs.set(m.result.sub, { view, last: m.result.snapshot });
        m.result.snapshot = patch(view, structuredClone(m.result.snapshot));
      } else if ("sub" in m && Array.isArray(m.updates) && subs.has(m.sub)) {
        const s = subs.get(m.sub) as { view: string; last: Json };
        s.last = m.updates[m.updates.length - 1];
        m.updates = m.updates.map((u: Json) => patch(s.view, structuredClone(u)));
      }
      ws.send(JSON.stringify(m));
    });
    upstream.addEventListener("close", () => ws.close());
    ws.onMessage((raw) => {
      const m = JSON.parse(String(raw));
      if (m.op === "subscribe") asked.set(m.id, m.view);
      if (upstream.readyState === WebSocket.OPEN) upstream.send(String(raw));
      else early.push(String(raw));
    });
    ws.onClose(() => upstream.close());
  });
  /** Sends a record view's last snapshot (playback) again, through `patch` as it stands now. */
  const repush = (view: string) => {
    for (const [sub, s] of subs) if (s.view === view) page?.send(JSON.stringify({ sub, updates: [patch(view, structuredClone(s.last))] }));
  };
  return { context, repush };
}
