/** Settings > Stream choice (plans/m13-stream-choice.md 4.4): how Marquee
 * picks the version to play, one row per preference, and what it knows
 * about this device. Every word comes from apps/shared/stream-choice-words.json. */

import Monitor from "lucide-solid/icons/monitor";
import RotateCcw from "lucide-solid/icons/rotate-ccw";
import Smartphone from "lucide-solid/icons/smartphone";
import Tv from "lucide-solid/icons/tv";
import { createResource, createSignal, Match, Show, Switch } from "solid-js";
import { displayCaps, op, prefsView, reasonOf } from "../core/client";
import { debridAccounts } from "../core/debrid";
import type { Prefs } from "../core/generated/Prefs";
import { form } from "../form";
import { choiceDefaults, choiceWords, conditionsLine, languageName, screenClass, type StreamChoicePrefs } from "../lib/words";
import { Choice, LANGS } from "./settings";
import "./browse.css";
import "./settings.css";

/** The core's speed and space for this network (`streamConditions`). */
interface Measured {
  network: string;
  speedMbps: number | null;
  measured: boolean;
  freeBytes: number | null;
}

/** The network type as the browser reports it; most desktop browsers do not. */
export function networkType(): string {
  const type = (navigator as Navigator & { connection?: { type?: string } }).connection?.type;
  return type === "wifi" || type === "ethernet" || type === "cellular" ? type : "other";
}

const w = choiceWords.settings;
const labelled = <T,>(choices: { label: string; value: T }[]) => choices.map((c) => ({ value: c.value, label: c.label }));

export default function StreamChoice() {
  const prefs = prefsView();
  const streams = () => (prefs.state.data?.streams ?? null) as (Prefs["streams"] & StreamChoicePrefs) | null;
  const [error, setError] = createSignal<string | null>(null);
  const patch = (x: Record<string, unknown>) => {
    setError(null);
    op<Prefs>("setPrefs", { patch: { streams: x } }).catch((e) => setError(reasonOf(e)));
  };
  const toggle = (list: string[], v: string) => (list.includes(v) ? list.filter((x) => x !== v) : [...list, v]);
  const hasAccount = () => (debridAccounts()?.accounts.length ?? 0) > 0;

  // ponytail: a core without `streamConditions` (before phase 2) shows the
  // screen and the network only.
  const [measured, { refetch }] = createResource(() => op<Measured>("streamConditions").catch(() => null));
  const [measuring, setMeasuring] = createSignal(false);
  const measureAgain = () =>
    op("resetSpeed").then(
      () => {
        setMeasuring(true);
        void refetch();
      },
      (e) => setError(reasonOf(e)),
    );
  const caps = displayCaps();
  const device = () =>
    conditionsLine({
      screenClass: screenClass(caps.displayHeight),
      hdr: caps.hdr.length > 0,
      network: measured()?.network ?? networkType(),
      speedMbps: measured()?.speedMbps ?? null,
      speedMeasured: measured()?.measured ?? false,
      freeBytes: measured()?.freeBytes ?? null,
    });

  return (
    <main class="browse page settings-page column stream-choice" data-vscroll>
      <h1>{w.title}</h1>
      <p class="lede">{w.lede}</p>
      <Show when={error()}>
        <p class="st bad" role="status">
          <i class="dot" />
          {error()}
        </p>
      </Show>
      <Show when={streams()}>
        {(s) => (
          <div class="prefs">
            <Choice<number | null>
              autofocus
              label={w.size.label}
              group="choice-size"
              items={labelled(w.size.choices)}
              isOn={(v) => (s().sizeLimitBytes === undefined ? choiceDefaults.sizeLimitBytes : s().sizeLimitBytes) === v}
              onPick={(v) => patch({ sizeLimitBytes: v })}
            />
            <Choice<number>
              label={w.seeders.label}
              help={w.seeders.help}
              group="choice-seeders"
              items={labelled(w.seeders.choices)}
              isOn={(v) => (s().minSeeders ?? choiceDefaults.minSeeders) === v}
              onPick={(v) => patch({ minSeeders: v })}
            />
            <Choice<number | null>
              label={w.resolution.label}
              group="choice-height"
              items={labelled(w.resolution.choices)}
              // A 4K cap is this screen's on every screen the apps run on.
              isOn={(v) => (v === null ? s().maxHeight === null || (s().maxHeight ?? 0) >= 2160 : s().maxHeight === v)}
              onPick={(v) => patch({ maxHeight: v })}
            />
            <Choice<boolean>
              label={w.hdr.label}
              group="choice-hdr"
              items={labelled(w.hdr.choices)}
              isOn={(v) => (s().hdr ?? choiceDefaults.hdr) === v}
              onPick={(v) => patch({ hdr: v })}
            />
            <Show when={hasAccount()}>
              <Choice<boolean>
                label={w.ready.label}
                group="choice-ready"
                items={labelled(w.ready.choices)}
                isOn={(v) => (s().preferReady ?? choiceDefaults.preferReady) === v}
                onPick={(v) => patch({ preferReady: v })}
              />
            </Show>
            <Choice<string>
              multi
              label={w.audio.label}
              help={w.audio.help}
              group="choice-audio"
              items={LANGS.map(([iso1]) => ({ value: iso1, label: languageName(iso1) }))}
              isOn={(v) => s().languages.includes(v)}
              onPick={(v) => patch({ languages: toggle(s().languages, v) })}
            />
            <Choice<boolean>
              label={w.learn.label}
              group="choice-learn"
              items={labelled(w.learn.choices)}
              isOn={(v) => (s().learn ?? choiceDefaults.learn) === v}
              onPick={(v) => patch({ learn: v })}
            />
            <div class="chips-row wrap-row device-row" data-fgroup="choice-device">
              <span class="kicker">{w.device.label}</span>
              <div class="device-line">
                <Switch>
                  <Match when={form() === "tv"}>
                    <Tv aria-hidden />
                  </Match>
                  <Match when={form() === "phone"}>
                    <Smartphone aria-hidden />
                  </Match>
                  <Match when={true}>
                    <Monitor aria-hidden />
                  </Match>
                </Switch>
                <span class="cond">{device()}</span>
                <Show when={measured()?.measured && !measuring()}>
                  <button class="btn" data-f="choice:measure" onClick={() => void measureAgain()}>
                    <RotateCcw aria-hidden />
                    {w.device.measureAgain}
                  </button>
                </Show>
                <Show when={measuring()}>
                  <span class="st idle" role="status">
                    <i class="dot" />
                    {w.device.measuring}
                  </span>
                </Show>
              </div>
            </div>
          </div>
        )}
      </Show>
    </main>
  );
}
