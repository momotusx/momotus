// Error reports from the web app (plans/error-reports.md 6.5): the page tells its core who it is
// (`reportContext`) and hands it uncaught errors that come from our own code. The core strips,
// queues, scrubs and uploads; this file never talks to the relay. webOS waits for its wasm core
// (step W), so nothing here runs there yet. The rules: lib/reports.ts.
import pkg from "../../package.json";
import { buildServer } from "./sync";
import { bridge } from "./client";
import { form } from "../form";
import { isWebOS } from "../lib/webos";
import { browserWords, jsReport, reportsWanted } from "../lib/reports";

/** Uncaught errors sent per page load, at most. */
const MAX_PER_LOAD = 10;
let sent = 0;

function report(error: unknown) {
  if (sent >= MAX_PER_LOAD) return;
  const r = jsReport(error, location.origin);
  if (!r) return;
  sent++;
  void bridge.request("report", { report: r }).catch(() => undefined);
}

if (reportsWanted(import.meta.env as { DEV?: boolean; VITE_MARQUEE_REPORTS?: string }, isWebOS())) {
  const { os, model } = browserWords(navigator.userAgent);
  const shown = form();
  void bridge
    .request("reportContext", {
      device: {
        app: "web",
        channel: import.meta.env.DEV ? "debug" : "release",
        version: pkg.version,
        os,
        model,
        form: shown === "tv" ? "tv" : shown === "phone" ? "phone" : "desktop",
        coreBuild: "own",
      },
      server: buildServer,
    })
    .catch(() => undefined);
  window.addEventListener("error", (e) => report(e.error));
  window.addEventListener("unhandledrejection", (e) => report(e.reason));
}
