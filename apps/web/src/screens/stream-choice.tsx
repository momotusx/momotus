/** Settings > Stream choice (plans/m13-stream-choice.md 4.4): how Marquee
 * picks the version to play, one row per preference, what the picks taught
 * (each with Undo), and what the core knows about this device. Every word
 * comes from apps/shared/stream-choice-words.json. */

import Monitor from "lucide-solid/icons/monitor";
import RotateCcw from "lucide-solid/icons/rotate-ccw";
import Smartphone from "lucide-solid/icons/smartphone";
import Tv from "lucide-solid/icons/tv";
import Undo2 from "lucide-solid/icons/undo-2";
import { createResource, createSignal, For, Match, Show, Switch } from "solid-js";
import { displayCaps, networkType, op, prefsView, reasonOf } from "../core/client";
import { debridAccounts } from "../core/debrid";
import type { Prefs } from "../core/generated/Prefs";
import type { StreamConditions } from "../core/generated/StreamConditions";
import { form } from "../form";
import { anyLearned, choiceWords, conditionsLine, forgetPatch, languageName, learnedRows, screenClass, speedFor, undoPatch } from "../lib/words";
import { Choice, LANGS } from "./settings";
import "./browse.css";
import "./settings.css";

const w = choiceWords.settings;
const labelled = <T,>(choices: { label: string; value: T }[]) => choices.map((c) => ({ value: c.value, label: c.label }));

export default function StreamChoice() {
  const prefs = prefsView();
  const p = () => prefs.state.data;
  const [error, setError] = createSignal<string | null>(null);
  const write = (x: Record<string, unknown>) => {
    setError(null);
    op<Prefs>("setPrefs", { patch: x }).catch((e) => setError(reasonOf(e)));
  };
  const patch = (x: Record<string, unknown>) => write({ streams: x });
  const toggle = (list: string[], v: string) => (list.includes(v) ? list.filter((x) => x !== v) : [...list, v]);
  const hasAccount = () => (debridAccounts()?.accounts.length ?? 0) > 0;
  // Undo and Forget take their row away: focus stays near, on the Learn
  // choice, never on Forget (a second press there would clear the rest).
  const keepFocus = () => document.querySelector<HTMLElement>('[data-fgroup="choice-learn"] [aria-checked="true"]')?.focus();
  const learnWrite = (x: Record<string, unknown>) => {
    if (document.activeElement?.closest(".learned")) keepFocus();
    write(x);
  };

  const [conditions, { mutate }] = createResource(() => op<StreamConditions>("streamConditions").catch(() => null));
  const [measuring, setMeasuring] = createSignal(false);
  const measured = () => {
    const c = conditions();
    return !!c && (c.http.samples > 0 || c.p2p.samples > 0);
  };
  const measureAgain = () =>
    op<StreamConditions>("resetSpeed").then(
      (c) => {
        // Measuring first: the button must never leave the page between the two.
        setMeasuring(true);
        mutate(c);
      },
      (e) => setError(reasonOf(e)),
    );
  const caps = displayCaps();
  const device = () => {
    const c = conditions();
    return conditionsLine({
      screenClass: screenClass(caps.displayHeight),
      hdr: caps.hdr.length > 0,
      network: c?.network ?? networkType(),
      speed: c ? speedFor(c, hasAccount()) : { mbps: 0, samples: 0 },
      freeBytes: c?.freeBytes ?? null,
    });
  };

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
      <Show when={p()}>
        {(x) => {
          const s = () => x().streams;
          return (
            <div class="prefs">
              <Choice<number | null>
                autofocus
                label={w.size.label}
                group="choice-size"
                items={labelled(w.size.choices)}
                isOn={(v) => s().sizeLimitBytes === v}
                onPick={(v) => patch({ sizeLimitBytes: v })}
              />
              <Choice<number>
                label={w.seeders.label}
                help={w.seeders.help}
                group="choice-seeders"
                items={labelled(w.seeders.choices)}
                isOn={(v) => s().minSeeders === v}
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
              <Choice<boolean> label={w.hdr.label} group="choice-hdr" items={labelled(w.hdr.choices)} isOn={(v) => s().hdr === v} onPick={(v) => patch({ hdr: v })} />
              <Show when={hasAccount()}>
                <Choice<boolean>
                  label={w.ready.label}
                  group="choice-ready"
                  items={labelled(w.ready.choices)}
                  isOn={(v) => s().preferReady === v}
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
              <Choice<boolean> label={w.learn.label} group="choice-learn" items={labelled(w.learn.choices)} isOn={(v) => s().learn === v} onPick={(v) => patch({ learn: v })} />
              <Show when={anyLearned(x())}>
                <div class="learned" data-fgroup="choice-learned">
                  <For each={learnedRows(x())}>
                    {(r) => (
                      <p class="lrow">
                        <span>{r.text}</span>
                        <button class="btn" data-f={`learned:${r.screenClass}:${r.choice}`} onClick={() => learnWrite(undoPatch(x(), r.screenClass, r.choice))}>
                          <Undo2 aria-hidden />
                          {choiceWords.learned.undo}
                        </button>
                      </p>
                    )}
                  </For>
                  <button class="btn" data-f="learned:forget" onClick={() => learnWrite(forgetPatch(x()))}>
                    <RotateCcw aria-hidden />
                    {w.learn.forget}
                  </button>
                </div>
              </Show>
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
                  {/* It stays after a press (spent), so focus is not lost. */}
                  <Show when={measured() || measuring()}>
                    <button class="btn" data-f="choice:measure" aria-disabled={measuring()} onClick={() => !measuring() && void measureAgain()}>
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
          );
        }}
      </Show>
    </main>
  );
}
