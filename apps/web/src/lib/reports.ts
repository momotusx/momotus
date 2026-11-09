// Error reports' rules for the page (plans/error-reports.md 6.5), without side effects so they test
// alone; core/reports.ts wires them to the core.
import { BridgeError } from "../core/bridge";

/** A stack line reduced to `file:line:col` when it points at one of this page's own files. */
export function ownFrame(line: string, origin: string): string | null {
  const at = line.indexOf(origin + "/");
  if (at < 0) return null;
  const rest = line.slice(at + origin.length + 1);
  const match = /([^/\s?#()]+\.(?:js|ts|tsx|mjs))(?:\?[^:\s)]*)?:(\d+):(\d+)/.exec(rest);
  return match ? `${match[1]}:${match[2]}:${match[3]}` : null;
}

/** The report the core takes for an uncaught error, or null: a failed op (every `BridgeError`, which
 * section 2 does not report), or an error with no frame of ours (an extension's script, a
 * cross-origin "Script error."). The text stays raw: the core's uploader scrubs it. */
export function jsReport(error: unknown, origin: string, now = Date.now()) {
  if (error instanceof BridgeError) return null;
  const stack = error instanceof Error && typeof error.stack === "string" ? error.stack.split("\n") : [];
  const frames = stack.map((l) => ownFrame(l, origin)).filter((f): f is string => f !== null);
  if (frames.length === 0) return null;
  const text = error instanceof Error ? `${error.name}: ${error.message}` : String(error);
  return { kind: "js", at: now, message: text.slice(0, 512), stack: frames.slice(0, 48) };
}

/** The browser and system in the core's charset: a family and a major version, never the agent string. */
export function browserWords(ua: string): { os: string; model: string } {
  const os = /Android/.test(ua)
    ? "Android"
    : /iPhone|iPad/.test(ua)
      ? "iOS"
      : /Mac OS X/.test(ua)
        ? "macOS"
        : /Windows/.test(ua)
          ? "Windows"
          : /Linux/.test(ua)
            ? "Linux"
            : "web";
  const found = /(Edg|Firefox|Chrome|Version)\/(\d+)/.exec(ua);
  const names: Record<string, string> = { Edg: "Edge", Firefox: "Firefox", Chrome: "Chrome", Version: "Safari" };
  return { os, model: found ? `${names[found[1]!]} ${found[2]}` : "Browser" };
}

/** On by default; development builds only with `VITE_MARQUEE_REPORTS=1` (a live check); never on
 * webOS until its wasm core lands (step W). */
export function reportsWanted(env: { DEV?: boolean; VITE_MARQUEE_REPORTS?: string }, webos: boolean): boolean {
  return !webos && (!env.DEV || env.VITE_MARQUEE_REPORTS === "1");
}
