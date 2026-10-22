/** The subtitle picker (plans/player-polish.md 2.4, design/DECISION.md
 * "Subtitle picker"): languages first, with a search field and flags, then
 * one language's options. TV shows both levels as two columns, the right one
 * the focused language's; desktop steps between them in one panel; the
 * phone is a full-height sheet. Picking a language turns on its best option
 * at once; its chevron (TV: Right) opens the options. */

import Check from "lucide-solid/icons/check";
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
import { bestOption, FITS, MATCHES, matchesLanguage, optionCount, pickerLanguages, type PickerLanguage, trackDetail } from "../lib/subtitles";
import { languageName } from "../lib/words";
import type { Track } from "./engine";
import { DelayStepper } from "./delay-stepper";
import { Flag } from "./flags";

/** A row of the tuning foot: its words, its value and the two steps (absent at an end). */
export type Tune = { text: string; less?: () => void; more?: () => void };

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
}) {
  const tv = () => form() === "tv";
  const [query, setQuery] = createSignal("");
  const languages = createMemo(() => pickerLanguages(props.tracks, props.fileSubs, languageName));
  const byTag = createMemo(() => new Map(languages().map((l) => [l.language, l])));
  // Rows go by language tag and options by URL, so a late addon adds to the
  // list without rebuilding the row a remote is on.
  const shown = createMemo(() => languages().filter((l) => matchesLanguage(l, query())).map((l) => l.language));
  const chosen = () => languages().find((l) => l.tracks.some((t) => t.url === props.on) || l.file.some((f) => f.id === props.onFile))?.language ?? null;
  // Off the TV the options show once a language is opened; on it, the right
  // column follows the focus, starting on the chosen language.
  const detail = () => byTag().get((tv() ? (props.open ?? chosen() ?? languages()[0]?.language) : props.open) ?? "") ?? null;
  const deep = () => !tv() && !!detail();

  const pickLanguage = (l: PickerLanguage<Track>) => {
    const best = bestOption(l);
    if (!best) return;
    if ("file" in best) props.pickFile(best.file.id);
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
              classList={{ on: !props.on && !props.onFile }}
              role="menuitemradio"
              aria-checked={!props.on && !props.onFile}
              aria-label={!props.on && !props.onFile ? "Off, selected" : "Off"}
              data-f="sub:off"
              data-autofocus
              onClick={() => props.pick(null)}
            >
              <span class="flag-space" aria-hidden="true" />
              <span class="lname">
                <span class="nm">Off</span>
              </span>
              <span class="ck">
                <Show when={!props.on && !props.onFile}>
                  <Check aria-hidden />
                </Show>
              </span>
            </button>
            <For each={shown()}>
              {(tag) => {
                const l = () => byTag().get(tag) as PickerLanguage<Track>;
                const on = () => chosen() === tag;
                return (
                  <div class="lrow">
                    <button
                      class="opt lr"
                      classList={{ on: on() }}
                      data-f={`sublang:${tag}`}
                      role="menuitemradio"
                      aria-checked={on()}
                      // Its own name too, and whether it is the one on: the check is drawn, not read.
                      aria-label={`${l().name}${l().own ? `, ${l().own}` : ""}, ${optionCount(l())} ${optionCount(l()) === 1 ? "option" : "options"}${on() ? ", selected" : ""}`}
                      onClick={() => pickLanguage(l())}
                      onFocus={() => tv() && followFocus(tag)}
                    >
                      <Flag language={tag} />
                      {/* The own name drops onto a hidden second line when both do not fit. */}
                      <span class="lname">
                        <span class="nm">{l().name}</span>
                        <Show when={l().own}>{(own) => <span class="own">{own()}</span>}</Show>
                      </span>
                      <span class="ct num">{optionCount(l())}</span>
                      <span class="ck">
                        <Show when={on()}>
                          <Check aria-hidden />
                        </Show>
                      </span>
                    </button>
                    <Show when={!tv()}>
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
                  const on = () => props.on === url;
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
                      onClick={() => props.pick(t())}
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
            {/* Timing for a subtitle out of step with the speech (per track, this play only), and the profile's size. */}
            <div class="subp-tune">
              <Show when={props.timing}>{(d) => <DelayStepper value={d().value} set={d().set} track={d().track} />}</Show>
              <div class="sub-size" role="group" aria-label="Subtitle size">
                <span>Size</span>
                <button class="sub-step" data-f="sub:smaller" aria-label="Smaller subtitles" disabled={!props.size.less} onClick={() => props.size.less?.()}>
                  <Minus aria-hidden />
                </button>
                <output class="num" aria-live="polite">
                  {props.size.text}
                </output>
                <button class="sub-step" data-f="sub:bigger" aria-label="Larger subtitles" disabled={!props.size.more} onClick={() => props.size.more?.()}>
                  <Plus aria-hidden />
                </button>
              </div>
            </div>
          </section>
        )}
      </Show>
    </aside>
  );
}
