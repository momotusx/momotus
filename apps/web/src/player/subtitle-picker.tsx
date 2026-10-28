/** The subtitle picker (plans/player-polish.md 2.4, design/DECISION.md
 * "Subtitle picker"): languages first, with a search field and flags, then
 * one language's options. TV shows both levels as two columns, the right one
 * the focused language's; desktop steps between them in one panel; the
 * phone is a full-height sheet. Picking a language turns on its best option
 * at once; its chevron (TV: Right) opens the options. */

import Check from "lucide-solid/icons/check";
import LoaderCircle from "lucide-solid/icons/loader-circle";
import ChevronLeft from "lucide-solid/icons/chevron-left";
import ChevronRight from "lucide-solid/icons/chevron-right";
import Minus from "lucide-solid/icons/minus";
import Plus from "lucide-solid/icons/plus";
import Search from "lucide-solid/icons/search";
import X from "lucide-solid/icons/x";
import { createEffect, createMemo, createSignal, For, on, onCleanup, Show } from "solid-js";
import type { SubtitleTrack } from "../core/generated/SubtitleTrack";
import { autoFocusSoon } from "../focus/dom";
import { form } from "../form";
import { bestOption, FITS, MATCHES, matchesLanguage, optionCount, pickerLanguages, type PickerLanguage, sameLanguage, secondLanguages, trackDetail } from "../lib/subtitles";
import { choiceWords, languageName } from "../lib/words";
import type { Track } from "./engine";
import { DelayStepper } from "./delay-stepper";
import { Flag } from "./flags";

/** A row of the tuning foot: its words, its value and the two steps (absent at an end). */
export type Tune = { text: string; less?: () => void; more?: () => void };

/** The second subtitle line (plans/dual-subtitles.md 5): its track, its language, the spoken language and
 * whether the Spoken language row is the choice, its timing, the profile's layout and Only when paused. */
export type SecondLine = {
  on: string | null;
  language: string | null;
  spoken: string | null;
  spokenOn: boolean;
  timing: { value: number; set: (sec: number) => void; track: string } | null;
  layout: Tune;
  onPause: { on: boolean; set: (on: boolean) => void };
  pick: (t: SubtitleTrack | null, spoken?: boolean) => void;
  keepSpoken: () => void;
};

const words = choiceWords.player;

/** How many characters a detail line holds: the phone sheet is narrower than the TV column and the desktop panel. */
const measure = () => (form() === "phone" ? 34 : 40);

export function SubtitlePicker(props: {
  tracks: SubtitleTrack[];
  /** The file's own subtitle tracks (the engine lists them once the file has opened). */
  fileSubs: Track[];
  /** The core's track that is on, by its URL. */
  on: string | null;
  /** The file's track that is on, by its id. */
  onFile: string | null;
  pending: boolean;
  notes: string[];
  /** The language whose options show: the second level off the TV, the right column on it. */
  open: string | null;
  setOpen: (language: string | null) => void;
  /** Timing, while a track is on; size, always. */
  /** The playing track's delay in seconds, or null when no core track plays. */
  timing: { value: number; set: (sec: number) => void; track: string } | null;
  size: Tune;
  pick: (t: SubtitleTrack | null) => void;
  pickFile: (id: string) => void;
  close: () => void;
  /** Which line the picker sets: the first (the subtitle), or the second (plans/dual-subtitles.md 5.1). */
  line: 1 | 2;
  setLine: (line: 1 | 2) => void;
  /** A first line is on (the switch shows then), tagged or not. */
  firstOn: boolean;
  /** The first line's language while one is on; null with subtitles off or for an untagged file track. */
  firstLanguage: string | null;
  second: SecondLine;
}) {
  const tv = () => form() === "tv";
  const [query, setQuery] = createSignal("");
  // The Second line tab: the core's languages only, the first line's disabled; the First line tab disables the
  // second's. Two lines in one language never happen by a pick.
  const two = () => props.line === 2 && props.firstOn;
  const languages = createMemo(() =>
    two()
      ? secondLanguages(props.tracks, props.firstLanguage, languageName)
      : pickerLanguages(props.tracks, props.fileSubs, languageName).map((l) => ({ ...l, disabled: sameLanguage(l.language, props.second.language) })),
  );
  const byTag = createMemo(() => new Map(languages().map((l) => [l.language, l])));
  // Rows go by language tag and options by URL, so a late addon adds to the
  // list without rebuilding the row a remote is on.
  const shown = createMemo(() => languages().filter((l) => matchesLanguage(l, query())).map((l) => l.language));
  const chosen = () =>
    two()
      ? props.second.spokenOn
        ? null
        : (languages().find((l) => l.tracks.some((t) => t.url === props.second.on))?.language ?? null)
      : (languages().find((l) => l.tracks.some((t) => t.url === props.on) || l.file.some((f) => f.id === props.onFile))?.language ?? null);
  // The Spoken language row with no track here is the choice, not Off.
  const offOn = () => (two() ? !props.second.on && !props.second.spokenOn : !props.on && !props.onFile);
  // The Spoken language row: the audio's language as the core reads it, pending while the list is still out.
  const spoken = () => (props.second.spoken ? (languages().find((l) => sameLanguage(l.language, props.second.spoken)) ?? null) : null);
  const spokenOwn = () => (props.second.spoken ? languageName(props.second.spoken) : props.pending ? null : words.spokenNotKnown);
  const spokenDisabled = () => sameLanguage(props.second.spoken, props.firstLanguage);
  const pickSpoken = () => {
    const best = spoken()?.tracks[0];
    if (best) props.second.pick(best, true);
    else props.second.keepSpoken();
  };
  const switchLine = (line: 1 | 2) => {
    clearTimeout(follow);
    props.setLine(line);
    props.setOpen(null);
    // A remote lands on the new tab's chosen row (plans/dual-subtitles.md 5.3); a keyboard stays on the switch.
    if (tv())
      autoFocusSoon(() => document.querySelector<HTMLElement>(".subp-langs .opt.on") ?? document.querySelector<HTMLElement>('[data-f="sub:off"]'));
  };
  // Off the TV the options show once a language is opened; on it, the right
  // column follows the focus, starting on the chosen language.
  // The other line's language never opens: none of its tracks can be picked here.
  const detail = () => {
    // An open language the other line has taken since falls back as no open one does.
    const open = props.open && !byTag().get(props.open)?.disabled ? props.open : null;
    const l = byTag().get((tv() ? (open ?? chosen() ?? languages().find((x) => !x.disabled)?.language) : open) ?? "");
    return l && !l.disabled ? l : null;
  };
  const deep = () => !tv() && !!detail();

  const pickLanguage = (l: PickerLanguage<Track>) => {
    const best = bestOption(l);
    if (!best) return;
    if (two()) {
      if ("track" in best) props.second.pick(best.track);
    } else if ("file" in best) props.pickFile(best.file.id);
    else props.pick(best.track);
  };
  // TV: the right column follows the focus once it rests (a held Down passes
  // over the rows without rebuilding the column at each).
  let follow: ReturnType<typeof setTimeout> | undefined;
  onCleanup(() => clearTimeout(follow));
  const followFocus = (language: string) => {
    clearTimeout(follow);
    follow = setTimeout(() => props.setOpen(language), 120);
  };
  // Off the TV the languages leave while a language's options show: they come back where they were.
  let langList: HTMLDivElement | undefined;
  let langScroll = 0;
  let search: HTMLInputElement | undefined;
  const openLanguage = (language: string) => {
    langScroll = langList?.scrollTop ?? 0;
    props.setOpen(language);
    autoFocusSoon(() => document.querySelector<HTMLElement>(".subp-opts .opt.on") ?? document.querySelector<HTMLElement>(".subp-opts .opt"));
  };
  const backToLanguages = () => {
    const was = props.open;
    props.setOpen(null);
    autoFocusSoon(() => document.querySelector<HTMLElement>(`[data-f="sublang:${was}"]`) ?? document.querySelector<HTMLElement>('[data-f="sub:off"]'));
  };
  // Once the languages are back (rendered in this same flush), before anyone can scroll them.
  createEffect(on(deep, (now, was) => was && !now && langList && (langList.scrollTop = langScroll), { defer: true }));
  const done = () => (
    <Show when={form() === "phone"}>
      <button class="subp-done" data-f="sub:done" onClick={() => props.close()}>
        Done
      </button>
    </Show>
  );

  return (
    <aside class="panel subp" classList={{ deep: deep() }} aria-label="Subtitles">
      <Show when={!deep()}>
        <section class="subp-col subp-langs" data-fgroup="sublangs" aria-label="Languages">
          <header class="subp-head">
            <h2>Subtitles</h2>
            {done()}
          </header>
          {/* First line / Second line (plans/dual-subtitles.md 5.1), while a first line is on. */}
          <Show when={props.firstOn}>
            <div class="subp-switch" role="radiogroup" aria-label="Subtitle line" data-fgroup="subline">
              <For each={[1, 2] as const}>
                {(line) => (
                  <button
                    class="seg"
                    classList={{ on: props.line === line }}
                    role="radio"
                    aria-checked={props.line === line}
                    data-f={`sub:line${line}`}
                    onClick={() => switchLine(line)}
                    // The switch's ends stop a remote: Left from the first and Right from the last stay in the panel.
                    onKeyDown={(e) => {
                      if ((line === 1 && e.key === "ArrowLeft") || (line === 2 && e.key === "ArrowRight")) e.preventDefault();
                    }}
                  >
                    {line === 1 ? words.firstLine : words.secondLine}
                  </button>
                )}
              </For>
            </div>
          </Show>
          <div class="subp-find">
            <Search aria-hidden />
            <input
              ref={search}
              type="search"
              data-f="sub:search"
              placeholder="Search languages"
              aria-label="Search languages"
              autocomplete="off"
              spellcheck={false}
              value={query()}
              onInput={(e) => setQuery(e.currentTarget.value)}
            />
            {/* A touch or a pointer clears it in one press; a remote has its keyboard's delete. */}
            <Show when={!tv() && query()}>
              <button
                class="subp-clear"
                data-f="sub:clear"
                aria-label="Clear the search"
                onClick={() => {
                  setQuery("");
                  search?.focus();
                }}
              >
                <X aria-hidden />
              </button>
            </Show>
          </div>
          <div
            class="subp-list"
            data-vscroll
            role="group"
            aria-label="Languages"
            ref={langList}
          >
            <button
              class="opt lr"
              classList={{ on: offOn() }}
              role="menuitemradio"
              aria-checked={offOn()}
              aria-label={offOn() ? "Off, selected" : "Off"}
              data-f="sub:off"
              data-autofocus
              onClick={() => (two() ? props.second.pick(null) : props.pick(null))}
            >
              <span class="flag-space" aria-hidden="true" />
              <span class="lname">
                <span class="nm">Off</span>
              </span>
              <span class="ck">
                <Show when={offOn()}>
                  <Check aria-hidden />
                </Show>
              </span>
            </button>
            <Show when={two() && !query().trim()}>
              <div class="lrow">
                <button
                  class="opt lr"
                  classList={{ on: props.second.spokenOn }}
                  role="menuitemradio"
                  aria-checked={props.second.spokenOn}
                  aria-disabled={spokenDisabled() || undefined}
                  disabled={spokenDisabled()}
                  aria-label={[words.spokenLanguage, spokenDisabled() ? words.onFirstLine : (spokenOwn() ?? "still searching"), ...(props.second.spokenOn ? ["selected"] : [])].filter(Boolean).join(", ")}
                  data-f="sub:spoken"
                  onClick={pickSpoken}
                  onFocus={() => tv() && followFocus(spoken()?.language ?? "")}
                >
                  <Show when={props.second.spoken} fallback={<span class="flag-space" aria-hidden="true" />}>
                    {(tag) => <Flag language={tag()} />}
                  </Show>
                  <span class="lname">
                    <span class="nm">{words.spokenLanguage}</span>
                    <Show when={spokenDisabled() ? words.onFirstLine : spokenOwn()} fallback={<LoaderCircle class="spin mark" role="img" aria-label="Still searching" />}>
                      {(own) => <span class="own">{own()}</span>}
                    </Show>
                  </span>
                  <span class="ck">
                    <Show when={props.second.spokenOn}>
                      <Check aria-hidden />
                    </Show>
                  </span>
                </button>
                <Show when={!tv() && spoken() && !spokenDisabled()}>
                  <button class="lr-open" data-f="subopen:spoken" aria-label={`${words.spokenLanguage} options`} onClick={() => openLanguage(spoken()?.language ?? "")}>
                    <ChevronRight aria-hidden />
                  </button>
                </Show>
              </div>
            </Show>
            <For each={shown()}>
              {(tag) => {
                const l = () => byTag().get(tag) as PickerLanguage<Track> & { disabled: boolean };
                const on = () => chosen() === tag;
                // The other line's language: shown, never picked here.
                const other = () => (l().disabled ? (two() ? words.onFirstLine : words.onSecondLine) : null);
                return (
                  <div class="lrow">
                    <button
                      class="opt lr"
                      classList={{ on: on() }}
                      data-f={`sublang:${tag}`}
                      role="menuitemradio"
                      aria-checked={on()}
                      aria-disabled={l().disabled || undefined}
                      disabled={l().disabled}
                      // Its own name too, and whether it is the one on: the check is drawn, not read.
                      aria-label={`${l().name}${other() ? `, ${other()}` : l().own ? `, ${l().own}` : ""}, ${optionCount(l())} ${optionCount(l()) === 1 ? "option" : "options"}${on() ? ", selected" : ""}`}
                      onClick={() => pickLanguage(l())}
                      onFocus={() => tv() && followFocus(tag)}
                    >
                      <Flag language={tag} />
                      {/* The own name drops onto a hidden second line when both do not fit. */}
                      <span class="lname">
                        <span class="nm">{l().name}</span>
                        <Show when={other() ?? l().own}>{(own) => <span class="own">{own()}</span>}</Show>
                      </span>
                      <span class="ct num">{optionCount(l())}</span>
                      <span class="ck">
                        <Show when={on()}>
                          <Check aria-hidden />
                        </Show>
                      </span>
                    </button>
                    <Show when={!tv() && !l().disabled}>
                      <button class="lr-open" data-f={`subopen:${tag}`} aria-label={`${l().name} options`} onClick={() => openLanguage(tag)}>
                        <ChevronRight aria-hidden />
                      </button>
                    </Show>
                  </div>
                );
              }}
            </For>
            <Show when={query().trim() && !shown().length}>
              <p class="subp-none">No language matches “{query().trim()}”</p>
            </Show>
            <Show when={props.pending}>
              <p class="opt-note">Still asking subtitle addons</p>
            </Show>
            <For each={props.notes}>{(n) => <p class="opt-note">{n}</p>}</For>
          </div>
        </section>
      </Show>
      <Show when={detail()}>
        {(l) => (
          <section class="subp-col subp-opts" data-fgroup="subopts" aria-label={l().name}>
            <header class="subp-head">
              <Show when={!tv()}>
                <button class="subp-back" data-f="sub:languages" onClick={backToLanguages}>
                  <ChevronLeft aria-hidden />
                  Languages
                </button>
              </Show>
              <h2>
                <Show when={form() === "desktop"}>
                  <Flag language={l().language} />
                </Show>
                {l().name}
              </h2>
              {done()}
            </header>
            <div class="subp-list" data-vscroll role="group" aria-label={`${l().name} options`}>
              {/* The file's own tracks first: they always fit the file. */}
              <For each={l().file.map((f) => f.id)}>
                {(id) => {
                  const f = () => l().file.find((x) => x.id === id) as Track;
                  const on = () => props.onFile === id;
                  return (
                    <button
                      class="opt op"
                      classList={{ on: on() }}
                      role="menuitemradio"
                      aria-checked={on()}
                      aria-label={`${f().label && f().label !== f().lang ? f().label : l().name}, ${f().forced ? "In the file, Forced" : "In the file"}${on() ? ", selected" : ""}`}
                      data-f={`subfile:${id}`}
                      onClick={() => props.pickFile(id)}
                    >
                      <span class="op-tx">
                        <b>{f().label && f().label !== f().lang ? f().label : l().name}</b>
                        <small>{f().forced ? "In the file, Forced" : "In the file"}</small>
                      </span>
                      <span class="ck">
                        <Show when={on()}>
                          <Check aria-hidden />
                        </Show>
                      </span>
                    </button>
                  );
                }}
              </For>
              <For each={l().tracks.map((t) => t.url)}>
                {(url) => {
                  const t = () => l().tracks.find((x) => x.url === url) as SubtitleTrack;
                  const on = () => (two() ? props.second.on === url : props.on === url);
                  const parts = () => trackDetail(t(), measure());
                  const release = () => !!t().release && t().release !== t().name;
                  return (
                    <button
                      class="opt op"
                      classList={{ on: on() }}
                      role="menuitemradio"
                      aria-checked={on()}
                      aria-label={[t().name, ...parts(), ...(on() ? ["selected"] : [])].join(", ")}
                      data-f={`sub:${url}`}
                      onClick={() => (two() ? props.second.pick(t()) : props.pick(t()))}
                    >
                      <span class="op-tx">
                        <b>{t().name}</b>
                        <Show when={parts().length}>
                          <small>
                            <For each={parts()}>
                              {(p, i) => (
                                <>
                                  {/* A line that wraps starts with the separator, never ends on one. */}
                                  {i() ? " " : ""}
                                  <span class="part">
                                    <Show when={i()}>
                                      <span class="sep">· </span>
                                    </Show>
                                    <span classList={{ fit: p === FITS || p === MATCHES, rel: release() && i() === parts().length - 1 }} title={release() && i() === parts().length - 1 ? (t().release ?? undefined) : undefined}>
                                      {p}
                                    </span>
                                  </span>
                                </>
                              )}
                            </For>
                          </small>
                        </Show>
                      </span>
                      <span class="ck">
                        <Show when={on()}>
                          <Check aria-hidden />
                        </Show>
                      </span>
                    </button>
                  );
                }}
              </For>
            </div>
            {/* Timing for a subtitle out of step with the speech (per track, this play only), and the profile's size;
                on the Second line tab also its layout and Only when paused (plans/dual-subtitles.md 5.1). */}
            <div class="subp-tune">
              <Show when={two() ? props.second.timing : props.timing}>{(d) => <DelayStepper value={d().value} set={d().set} track={d().track} />}</Show>
              <Stepper label="Size" name="Subtitle size" text={props.size.text} tune={props.size} less={["sub:smaller", "Smaller subtitles"]} more={["sub:bigger", "Larger subtitles"]} />
              <Show when={two()}>
                <Stepper label={words.layout} name={words.layout} text={props.second.layout.text} tune={props.second.layout} less={["sub:layout:less", "Previous layout"]} more={["sub:layout:more", "Next layout"]} />
                <button class="sub-size sub-switch" role="switch" aria-checked={props.second.onPause.on} data-f="sub:paused" onClick={() => props.second.onPause.set(!props.second.onPause.on)}>
                  <span>{words.onlyWhenPaused}</span>
                  <span class="knob" aria-hidden="true">
                    <i />
                  </span>
                </button>
              </Show>
            </div>
          </section>
        )}
      </Show>
    </aside>
  );
}

/** A tuning row: its words, minus, the value and plus (Size, and the second line's Layout); each step is
 * its focus key and its spoken name. */
function Stepper(props: { label: string; name: string; text: string; tune: Tune; less: [string, string]; more: [string, string] }) {
  return (
    <div class="sub-size" role="group" aria-label={props.name}>
      <span>{props.label}</span>
      <button class="sub-step" data-f={props.less[0]} aria-label={props.less[1]} disabled={!props.tune.less} onClick={() => props.tune.less?.()}>
        <Minus aria-hidden />
      </button>
      <output class="num" aria-live="polite">
        {props.text}
      </output>
      <button class="sub-step" data-f={props.more[0]} aria-label={props.more[1]} disabled={!props.tune.more} onClick={() => props.tune.more?.()}>
        <Plus aria-hidden />
      </button>
    </div>
  );
}
