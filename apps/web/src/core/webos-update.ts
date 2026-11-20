/** Updates on a webOS TV (plans/auto-update.md section 2): the app's service checks the signed feed, downloads the
 * IPK and installs it on the user's Install; this page shows its state (Settings, About; Home's quiet line) and asks
 * for the checks nobody pressed for, at start and once a day, only while no video plays. */

import { createSignal } from "solid-js";
import words from "../../../shared/update-words.json";
import { listenUpdates, sendUpdate } from "./webos-transport";

export const updateWords = words;

export type UpdateState =
  | { kind: "idle" | "checking" | "current" | "failed" }
  | { kind: "downloading"; version: string; percent: number }
  | { kind: "ready" | "installing"; version: string };

const DAY_MS = 24 * 60 * 60 * 1000;
const SPREAD_MS = 2 * 60 * 60 * 1000;

/** null: no updates here (not webOS, or a service without the feed key). */
const [state, setState] = createSignal<UpdateState | null>(null);
export const updateState = state;

/** Every answer and every pushed state carries the whole state. */
export function takeUpdate(text: string) {
  try {
    const m = JSON.parse(text) as { state?: UpdateState | null };
    if (m.state !== undefined) setState(m.state);
  } catch {
    // Not ours to read.
  }
}

const playing = () => Array.from(document.querySelectorAll("video")).some((v) => !v.paused);

export const checkUpdates = () => void sendUpdate({ op: "check", asked: true });
export const installUpdate = () => void sendUpdate({ op: "install" });

/** Once, from the webOS client's start. */
export function startUpdates() {
  listenUpdates(takeUpdate);
  sendUpdate({ op: "state" });
  const quietCheck = () => {
    if (!playing()) sendUpdate({ op: "check", asked: false });
  };
  quietCheck();
  const again = () =>
    setTimeout(() => {
      quietCheck();
      again();
    }, DAY_MS + Math.floor(Math.random() * SPREAD_MS));
  again();
}

export const fillUpdate = (word: string, s: UpdateState) =>
  word.replace("{version}", "version" in s ? s.version : "").replace("{percent}", "percent" in s ? String(s.percent) : "");

/** The state's line, and the action it offers (none while it works). */
export function updateRow(s: UpdateState): { line: string; action: { label: string; run: () => void; install: boolean } | null } {
  const check = { label: words.check, run: checkUpdates, install: false };
  switch (s.kind) {
    case "idle":
      return { line: "", action: check };
    case "current":
      return { line: words.current, action: check };
    case "failed":
      return { line: words.failed, action: check };
    case "checking":
      return { line: words.checking, action: null };
    case "downloading":
      return { line: fillUpdate(words.downloadingPercent, s), action: null };
    case "ready":
      return { line: fillUpdate(words.ready, s), action: { label: words.install, run: installUpdate, install: true } };
    case "installing":
      return { line: fillUpdate(words.installing, s), action: null };
  }
}
