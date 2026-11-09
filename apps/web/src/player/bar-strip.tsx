/** The player bar's quick subtitle shift and its two strips (plans/player-bar.md 5, round3.html variant 1
 * "Toolbar strip").
 *
 * The shift: Earlier, the value and Later in one capsule beside Subtitles while a core subtitle is on (D4). A press
 * moves 0.25 s, a hold repeats faster each second (lib/delay.ts QUICK), and pressing the value resets it. On TV the
 * value is the one focus stop: OK enters adjust mode, where Left and Right shift by the same rules, held or not;
 * OK, Up and Down leave it (Back too, through the player), and so does focus leaving.
 *
 * The subtitle strip opens over the bar's right end: its languages as chips (D5), More languages (today's full
 * picker), the shift, Style (a row above: size, position, background) and Close. The audio strip mirrors it over
 * the left with the tracks and Choose a version. */

import AudioLines from "lucide-solid/icons/audio-lines";
import Captions from "lucide-solid/icons/captions";
import ChevronDown from "lucide-solid/icons/chevron-down";
import ChevronLeft from "lucide-solid/icons/chevron-left";
import ChevronRight from "lucide-solid/icons/chevron-right";
import ChevronUp from "lucide-solid/icons/chevron-up";
import LoaderCircle from "lucide-solid/icons/loader-circle";
import Minus from "lucide-solid/icons/minus";
import Plus from "lucide-solid/icons/plus";
import Type from "lucide-solid/icons/type";
import X from "lucide-solid/icons/x";
import { createEffect, createMemo, For, type JSX, on, onCleanup, Show } from "solid-js";
import type { SubtitleBackground } from "../core/generated/SubtitleBackground";
import { form } from "../form";
import { barWords, shiftName } from "../lib/bar";
import { delayText, hold, nudge, QUICK, QUICK_STEP } from "../lib/delay";
import { Flag } from "./flags";

const names = barWords.names;

/** Which shift a key names: the bar's or the strip's (both drive the same value). */
export type ShiftAt = "bar" | "strip";

export function QuickShift(props: {
  at: ShiftAt;
  value: number;
  set: (sec: number) => void;
  /** The track the value is for: a new one ends a hold under way. */
  track: string;
  /** TV adjust mode, held by the player so Back can leave it first. */
  adjusting: boolean;
  setAdjusting: (on: boolean) => void;
  /** A hold starts (true) or ends (false): the controls never hide meanwhile. */
  onHold: (on: boolean) => void;
}) {
  const tv = () => form() === "tv";
  // Read at each tick: a hold moves from wherever the last tick left the value.
  const rule = hold((by) => props.set(nudge(props.value, by)), undefined, QUICK);
  let holder: EventTarget | null = null;
  const stop = (single: boolean) => {
    const was = rule.held();
    rule.stop(single);
    holder = null;
    if (was) props.onHold(false);
  };
  const start = (d: number, el: EventTarget | null) => {
    holder = el;
    rule.start(d);
    props.onHold(true);
  };
  const track = createMemo(() => props.track);
  createEffect(on(track, () => stop(false), { defer: true }));
  onCleanup(() => stop(false));
  const blur = (e: FocusEvent) => e.currentTarget === holder && stop(false);

  /** Earlier or Later: the pointer, Enter and Space hold by the quick rule (not the keyboard's own repeat); a click
   * with no press behind it (a screen reader's) is one step. */
  const holds = (d: number) => ({
    onPointerDown: (e: PointerEvent & { currentTarget: HTMLElement }) => {
      if (e.button !== 0) return;
      e.currentTarget.setPointerCapture(e.pointerId);
      start(d, e.currentTarget);
    },
    onPointerUp: () => stop(true),
    onPointerCancel: () => stop(false),
    onLostPointerCapture: (e: PointerEvent) => e.currentTarget === holder && stop(false),
    onContextMenu: (e: MouseEvent) => e.preventDefault(),
    onKeyDown: (e: KeyboardEvent) => {
      if (e.key !== "Enter" && e.key !== " ") return;
      e.preventDefault();
      if (!e.repeat) start(d, e.currentTarget);
    },
    onKeyUp: (e: KeyboardEvent) => {
      if (e.key !== "Enter" && e.key !== " ") return;
      e.preventDefault();
      stop(true);
    },
    onBlur: blur,
    onClick: (e: MouseEvent) => e.detail === 0 && props.set(nudge(props.value, d * QUICK_STEP)),
  });
  const side = (d: -1 | 1) => (
    <button
      class="qsb"
      data-f={tv() ? undefined : `${props.at}:${d < 0 ? "earlier" : "later"}`}
      tabIndex={tv() ? -1 : undefined}
      aria-label={d < 0 ? names.earlier : names.later}
      title={form() === "desktop" ? `${d < 0 ? names.earlier : names.later} (${d < 0 ? "G" : "H"})` : undefined}
      {...holds(d)}
    >
      {/* The words, or chevrons where the bar is tight (player.css, data-chev): the name stays. */}
      <span class="w">{d < 0 ? barWords.earlier : barWords.later}</span>
      {d < 0 ? <ChevronLeft class="c" aria-hidden /> : <ChevronRight class="c" aria-hidden />}
    </button>
  );

  return (
    <div class="qs" classList={{ adj: props.adjusting }} role="group" aria-label={names.timing}>
      {side(-1)}
      <button
        class="qv num"
        data-f={`${props.at}:shift`}
        aria-label={shiftName(props.value, tv())}
        onClick={() => (tv() ? props.setAdjusting(!props.adjusting) : props.value !== 0 && props.set(0))}
        onKeyDown={(e) => {
          // A held OK toggles once, not at each of the remote's repeats.
          if (e.key === "Enter" && e.repeat) return e.preventDefault();
          if (!tv() || !props.adjusting) return;
          if (e.key === "ArrowLeft" || e.key === "ArrowRight") {
            // Adjust mode owns Left and Right; the remote's own repeats are swallowed (section 6, held keys).
            e.preventDefault();
            e.stopPropagation();
            if (!e.repeat) start(e.key === "ArrowLeft" ? -1 : 1, e.currentTarget);
          } else if (e.key === "ArrowUp" || e.key === "ArrowDown") {
            // Up and Down leave adjust mode only: the focus stays on the value.
            e.preventDefault();
            e.stopPropagation();
            stop(false);
            props.setAdjusting(false);
          }
        }}
        onKeyUp={(e) => {
          if (tv() && (e.key === "ArrowLeft" || e.key === "ArrowRight")) stop(true);
        }}
        onBlur={(e) => {
          blur(e);
          props.setAdjusting(false);
        }}
      >
        <Show when={tv()}>
          <ChevronLeft class="ca" aria-hidden />
        </Show>
        {delayText(props.value)}
        <Show when={tv()}>
          <ChevronRight class="ca" aria-hidden />
        </Show>
      </button>
      {side(1)}
    </div>
  );
}

/** A stepper of the Style row: its label, minus, the value and plus (absent ends disabled). */
type Tune = { text: string; less?: () => void; more?: () => void };
function Stepper(props: { label: string; tune: Tune; keys: [string, string]; words: [string, string]; icons: [JSX.Element, JSX.Element]; note?: string }) {
  return (
    <span class="si" role="group" aria-label={props.label}>
      <span class="slb">{props.label}</span>
      <Show when={!props.note} fallback={<span class="snote">{props.note}</span>}>
        <button class="stb" data-f={props.keys[0]} aria-label={props.words[0]} disabled={!props.tune.less} onClick={() => props.tune.less?.()}>
          {props.icons[0]}
        </button>
        <output aria-live="polite">{props.tune.text}</output>
        <button class="stb" data-f={props.keys[1]} aria-label={props.words[1]} disabled={!props.tune.more} onClick={() => props.tune.more?.()}>
          {props.icons[1]}
        </button>
      </Show>
    </span>
  );
}

const BACKGROUNDS: SubtitleBackground[] = ["box", "outline", "shadow"];

export type SubtitleStripProps = {
  /** The languages after Off (lib/bar.ts stripLanguages). */
  languages: { language: string; name: string }[];
  /** The language on now, or null for Off. */
  checked: string | null;
  pick: (language: string | null) => void;
  /** The core still asks and nothing is listed yet. */
  searching: boolean;
  more: () => void;
  shift: { value: number; set: (sec: number) => void; track: string } | null;
  adjusting: boolean;
  setAdjusting: (on: boolean) => void;
  onHold: (on: boolean) => void;
  style: boolean;
  toggleStyle: () => void;
  size: Tune;
  position: Tune;
  /** Why Position does nothing here (an upright phone draws in the band under the picture). */
  positionNote?: string;
  background: { value: SubtitleBackground; set: (b: SubtitleBackground) => void };
  close: () => void;
};

const closeButton = (close: () => void) => (
  <button class="qb sx" data-f="strip:close" aria-label={barWords.close} title={barWords.close} onClick={close}>
    <X aria-hidden />
  </button>
);

export function SubtitleStrip(props: SubtitleStripProps) {
  const tv = () => form() === "tv";
  return (
    <div class="bstrip sub" role="region" aria-label={names.subtitles} data-fgroup="strip">
      <div class="bsrow s1">
        <span class="slb">
          <Captions aria-hidden />
          {names.subtitles}
        </span>
        <div class="strk" role="radiogroup" aria-label={names.subtitleLanguage}>
          <button class="chip3" role="radio" aria-checked={props.checked === null} data-f="strip:sub:off" onClick={() => props.pick(null)}>
            <svg class="fs" viewBox="0 0 26 26" aria-hidden="true">
              <circle cx="13" cy="13" r="12" fill="none" stroke="currentColor" stroke-opacity=".3" stroke-width="2" />
            </svg>
            <span class="cl">Off</span>
          </button>
          <For each={props.languages}>
            {(l) => (
              <button class="chip3" role="radio" aria-checked={props.checked === l.language} data-f={`strip:sub:${l.language}`} onClick={() => props.pick(l.language)}>
                <Flag language={l.language} />
                <span class="cl">{l.name}</span>
              </button>
            )}
          </For>
          <Show when={props.searching}>
            <span class="snote" role="status">
              <LoaderCircle class="spin" aria-hidden />
              Still searching
            </span>
          </Show>
          <button class="chip3 ghost" data-f="strip:more" onClick={() => props.more()}>
            <span class="cl">{barWords.moreLanguages}</span>
            <ChevronRight aria-hidden />
          </button>
        </div>
        <Show when={props.shift}>
          {(s) => (
            <>
              <span class="svr" aria-hidden="true" />
              <QuickShift at="strip" value={s().value} set={s().set} track={s().track} adjusting={props.adjusting} setAdjusting={props.setAdjusting} onHold={props.onHold} />
              {/* D22: a remote cannot press the value to reset (OK adjusts), so TV has Reset while it is not 0. */}
              <Show when={tv() && s().value !== 0}>
                <button class="chip3 ghost" data-f="strip:reset" onClick={() => s().set(0)}>
                  <span class="cl">{barWords.reset}</span>
                </button>
              </Show>
            </>
          )}
        </Show>
        <span class="svr" aria-hidden="true" />
        <button class="chip3 ghost" data-f="strip:style" aria-expanded={props.style} onClick={() => props.toggleStyle()}>
          <Type aria-hidden />
          <span class="cl">{barWords.style}</span>
        </button>
        {closeButton(props.close)}
      </div>
      <Show when={props.style}>
        <div class="bsrow s2">
          <Stepper label={barWords.size} tune={props.size} keys={["strip:smaller", "strip:larger"]} words={[names.smaller, names.larger]} icons={[<Minus aria-hidden />, <Plus aria-hidden />]} />
          <span class="svr" aria-hidden="true" />
          <Stepper
            label={barWords.position}
            tune={props.position}
            keys={["strip:lower", "strip:higher"]}
            words={[barWords.lower, barWords.higher]}
            icons={[<ChevronDown aria-hidden />, <ChevronUp aria-hidden />]}
            note={props.positionNote}
          />
          <span class="svr" aria-hidden="true" />
          <span class="si">
            <span class="slb" id="strip-bg">
              {barWords.background}
            </span>
            <span class="sseg" role="group" aria-labelledby="strip-bg">
              <For each={BACKGROUNDS}>
                {(b) => (
                  <button data-f={`strip:bg:${b}`} aria-pressed={props.background.value === b} onClick={() => props.background.set(b)}>
                    {barWords.backgrounds[b]}
                  </button>
                )}
              </For>
            </span>
          </span>
        </div>
      </Show>
    </div>
  );
}

export function AudioStrip(props: {
  tracks: { id: string; name: string; checked: boolean }[];
  pick: (id: string) => void;
  /** Choose a version (web's link to Versions), on every audio strip. */
  versions: JSX.Element;
  close: () => void;
}) {
  return (
    <div class="bstrip aud" role="region" aria-label={names.audio} data-fgroup="strip">
      <div class="bsrow s1">
        <span class="slb">
          <AudioLines aria-hidden />
          {names.audio}
        </span>
        <div class="strk" role="radiogroup" aria-label={names.audioTrack}>
          <For each={props.tracks}>
            {(t) => (
              <button class="chip3" role="radio" aria-checked={t.checked} data-f={`strip:aud:${t.id}`} onClick={() => props.pick(t.id)}>
                <AudioLines aria-hidden />
                <span class="cl">{t.name}</span>
              </button>
            )}
          </For>
          {props.versions}
        </div>
        {closeButton(props.close)}
      </div>
    </div>
  );
}

/** The strip's chip to land on when it opens or is re-entered: the checked one, else the first. */
export const stripEntry = () =>
  document.querySelector<HTMLElement>('.bstrip .chip3[aria-checked="true"]') ?? document.querySelector<HTMLElement>(".bstrip [data-f]");

/** Whether a strip's own control holds the focus. */
export const inStrip = (el: Element | null) => !!el?.closest(".bstrip");
