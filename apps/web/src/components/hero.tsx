/** Home's hero carousel (plans/appletv-polish.md 4.1 and 4.2): full-bleed pages that slide, the dots
 * with the active one a filling pill, auto-advance on input idle, and a muted trailer full bleed in the
 * hero's own frame, under the art until YouTube's start controls have faded. */
import { A } from "@solidjs/router";
import Info from "lucide-solid/icons/info";
import Pause from "lucide-solid/icons/pause";
import Play from "lucide-solid/icons/play";
import Volume2 from "lucide-solid/icons/volume-2";
import VolumeX from "lucide-solid/icons/volume-x";
import { createEffect, createSignal, For, on, onCleanup, onMount, Show, untrack } from "solid-js";
import tokens from "../../../../design/tokens.json";
import type { Hero } from "../core/generated/Hero";
import { form } from "../form";
import { reducedMotion } from "../focus/dom";
import { bezier, frameStep, HeroPager } from "../lib/hero-pager";
import { capableNow, coverSize, meteredNow, previewsOn, REVEAL_MS, TRAILER_LAYOUT, type TrailerLayout, trailerAllowed, walkNow as walk } from "../lib/trailers";
import { playPath, ratingParts, titlePath } from "../lib/format";
import { kindLabel } from "../lib/words";
import { Backdrop, Logo } from "./art";
import { ProfileButton } from "./avatar";
import { Ratings } from "./ratings";
import { separators } from "./separators";
import { muted, playTrailer, setMuted, stopTrailer, trailerPlaying } from "./yt-player";

// The owner's B2 (2026-10-24, design/tokens.json motion): a page stays 8 s, slides in 650 ms, its art moving at 0.85
// of its words.
const DWELL_MS = tokens.motion.heroDwellMs;
const SLIDE_MS = tokens.motion.heroSlideMs;
const PARALLAX = tokens.motion.heroParallax;
// The player starts loading as soon as a page has settled, under its art (plans/appletv-polish.md 4.2).
const TRAILER_DELAY_MS = 700;
// TV: how long after the last key the timed pages wait.
const KEY_HOLD_MS = 5000;
const TRAILER_CAP_S = 45;
const ease = bezier(0.22, 1, 0.36, 1);

/** The page shown, by title, across Back (Home remounts; its view stays). */
let shownId: string | null = null;
/** Pages whose trailer ended, failed or was cut by the viewer this session: they do not play again. */
const spent = new Set<string>();

/** The trailer's sound switch at the hero's bottom right, level with the dots (the owner, 2026-10-26): drawn only
 * while one plays, and in a walk only when the run asked for trailers. Its state is the session's (`muted`). */
function TrailerMute() {
  return (
    <Show when={trailerPlaying() && !walk()}>
      <button class="trailer-mute" aria-label="Trailer sound" aria-pressed={!muted()} onClick={() => setMuted(!muted())}>
        <Show when={muted()} fallback={<Volume2 aria-hidden />}>
          <VolumeX aria-hidden />
        </Show>
      </button>
    </Show>
  );
}

export function HeroCarousel(props: { heroes: Hero[]; waiting: boolean }) {
  // The list shown changes only between slides (decision 1), and an active page that left the core's
  // list stays until the next advance.
  const [list, setList] = createSignal<Hero[]>([]);
  const [index, setIndex] = createSignal(0);
  // Playing: the clock follows the trailer. Shown: the art above it has faded.
  const [trailerOn, setTrailerOn] = createSignal(false);
  const [shown, setShown] = createSignal(false);
  const [paused, setPaused] = createSignal(false);
  const pager = new HeroPager(0, DWELL_MS);
  let section!: HTMLElement;
  let track!: HTMLDivElement;
  let box!: HTMLDivElement;
  let fill: HTMLElement | null = null;
  let sliding = false;
  let manual = false;
  let hovered = false;
  let pressing = false;
  let visible = true;
  let pending: Hero[] | null = null;

  const apply = (next: Hero[]) => {
    const current = list()[index()]?.tile.id ?? shownId;
    const at = next.findIndex((h) => h.tile.id === current);
    if (current && at < 0 && list().some((h) => h.tile.id === current)) {
      pending = next;
      return;
    }
    pending = null;
    setList(next);
    pager.resize(next.length, Math.max(0, at));
    setIndex(pager.index);
    queueMicrotask(() => jump(pager.index));
  };
  createEffect(on(() => props.heroes.map((h) => h.tile.id).join(" "), () => (sliding ? (pending = [...props.heroes]) : apply([...props.heroes]))));

  const width = () => track?.clientWidth || 1;
  /** B2's depth: a page's art moves at PARALLAX of its words, so it lags `(1 - PARALLAX)` of the page's offset. */
  const depth = () => {
    if (!track || reducedMotion()) return;
    for (const page of track.querySelectorAll<HTMLElement>(".hero-page")) {
      const offset = page.offsetLeft - track.scrollLeft;
      if (Math.abs(offset) > width() * 1.5) continue;
      const art = page.querySelector<HTMLElement>(".hero-bd");
      if (art) art.style.transform = `translateX(${(-offset * (1 - PARALLAX)).toFixed(1)}px)`;
    }
  };
  const jump = (i: number) => {
    if (track) track.scrollLeft = i * width();
  };
  /** The timer's slide: the design's ease over SLIDE_MS, snapping off while it runs. */
  const slideTo = (i: number) => {
    stopTrailer();
    if (pending) return apply(pending);
    // Focus on a hero button goes with the page to the same button (Play to Play, else the first one), before the
    // old page goes inert: the TV hero slides while focus rests on it (the owner, 2026-10-26).
    const el = document.activeElement as HTMLElement | null;
    const held = el?.closest(".hero-page.on .acts") ? el.dataset.f : undefined;
    setIndex(i);
    if (held) refocus(held);
    if (reducedMotion()) return jump(i);
    const from = track.scrollLeft;
    const to = i * width();
    const start = performance.now();
    sliding = true;
    track.style.scrollSnapType = "none";
    const step = (now: number) => {
      const t = Math.min(1, (now - start) / SLIDE_MS);
      track.scrollLeft = from + (to - from) * ease(t);
      // The art follows in the same frame; the scroll event would place it a frame late.
      depth();
      if (t < 1) return void requestAnimationFrame(step);
      track.style.scrollSnapType = "";
      sliding = false;
      if (pending) apply(pending);
    };
    requestAnimationFrame(step);
  };
  // A page the viewer chose is said, in the status line under the carousel; the timer's never are (review r2, R2-3).
  const [said, setSaid] = createSignal("");
  const say = (i: number) => setSaid(`${i + 1} of ${list().length}, ${list()[i]?.tile.name ?? ""}`);
  const page = (i: number) => {
    if (i < 0 || i >= list().length || i === index()) return;
    pager.go(i, "manual");
    slideTo(i);
    say(i);
  };

  createEffect(() => {
    shownId = list()[index()]?.tile.id ?? shownId;
  });

  // Once anything on the screen has had the focus, a page's late Play no longer holds a first focus (data-focus-wait).
  const [landed, setLanded] = createSignal(false);
  onMount(() => {
    const land = () => setLanded(true);
    document.addEventListener("focusin", land, { once: true });
    onCleanup(() => document.removeEventListener("focusin", land));
  });

  // The clock: one progress value for the pill and the advance (decision 7), only while nothing holds it.
  let keyAt = -Infinity;
  onMount(() => {
    let last = performance.now();
    let written = -1;
    let frame = requestAnimationFrame(function loop(now) {
      const dt = frameStep(now, last);
      last = now;
      // TV: a key pressed holds the pages until 5 s after it, so nothing slides while the viewer moves.
      const keyHeld = form() === "tv" && now - keyAt < KEY_HOLD_MS;
      const running =
        visible && !document.hidden && !hovered && !pressing && !sliding && !keyHeld && !paused() && !trailerOn() && !reducedMotion() && untrack(list).length > 1;
      const next = pager.tick(dt, running);
      if (next !== null) slideTo(next);
      // The 24 px pill moves 3 px a second: a write each 1 percent is as smooth and spares the frames between.
      if (fill && Math.abs(pager.progress - written) >= 0.01) {
        written = pager.progress;
        fill.style.setProperty("--fill", String(written));
      }
      frame = requestAnimationFrame(loop);
    });
    onCleanup(() => cancelAnimationFrame(frame));
    // Any key, pointer move or touch starts the dwell over (input idle, not focus).
    const restart = (e: Event) => {
      pager.input();
      if (e.type === "keydown") keyAt = performance.now();
    };
    for (const kind of ["keydown", "pointermove", "touchstart"] as const) window.addEventListener(kind, restart, { passive: true, capture: true });
    onCleanup(() => {
      for (const kind of ["keydown", "pointermove", "touchstart"] as const) window.removeEventListener(kind, restart, { capture: true });
    });
    const seen = new IntersectionObserver(([e]) => {
      visible = !!e && e.intersectionRatio >= 0.5;
      if (!visible) stopTrailer();
    }, { threshold: [0, 0.5, 1] });
    seen.observe(section);
    // A page that settles in view by a swipe is the viewer's: the timer stops for the session.
    const pages = new IntersectionObserver(
      (entries) => {
        for (const e of entries) {
          if (e.intersectionRatio < 0.6 || sliding) continue;
          const i = Number((e.target as HTMLElement).dataset.page);
          if (i === index()) continue;
          if (manual) {
            pager.go(i, "manual");
            say(i);
          }
          stopTrailer();
          setIndex(i);
          manual = false;
        }
      },
      { root: track, threshold: [0.6] },
    );
    createEffect(() => {
      list();
      pages.disconnect();
      queueMicrotask(() => track.querySelectorAll<HTMLElement>(".hero-page").forEach((p) => pages.observe(p)));
    });
    const hide = () => document.hidden && stopTrailer();
    document.addEventListener("visibilitychange", hide);
    onCleanup(() => {
      seen.disconnect();
      pages.disconnect();
      document.removeEventListener("visibilitychange", hide);
      stopTrailer();
    });
  });

  // The trailer: TRAILER_DELAY_MS after a page settles, when every condition holds (decision 5).
  const layout = (): TrailerLayout => TRAILER_LAYOUT[form()];
  // Keyed by the page shown, so the list growing never restarts or hides a trailer that plays.
  createEffect(
    on(
      () => list()[index()]?.tile.id,
      () => {
      setTrailerOn(false);
      setShown(false);
      const hero = list()[index()];
      if (!hero) return;
      const timer = setTimeout(() => start(hero), TRAILER_DELAY_MS);
      onCleanup(() => clearTimeout(timer));
      },
    ),
  );
  const start = (hero: Hero) => {
    const signals = {
      enabled: previewsOn(),
      visible: visible && !document.hidden,
      reducedMotion: reducedMotion(),
      metered: meteredNow(),
      walk: walk(),
      layout: layout(),
      capable: capableNow(),
      hasTrailer: !!hero.trailer && !spent.has(hero.tile.id),
    };
    if (!trailerAllowed(signals) || !hero.trailer) return;
    // The player covers the whole frame, oversized so YouTube's bars fall outside the clip.
    const size = coverSize(section.offsetWidth, section.offsetHeight);
    Object.assign(box.style, { width: `${size.width}px`, height: `${size.height}px` });
    let reveal: ReturnType<typeof setTimeout> | undefined;
    playTrailer({
      host: box,
      id: hero.trailer,
      capS: TRAILER_CAP_S,
      onPlaying: () => {
        setTrailerOn(true);
        reveal = setTimeout(() => setShown(true), REVEAL_MS);
      },
      onTime: (f) => pager.trailer(f),
      onEnd: (why) => {
        clearTimeout(reveal);
        setShown(false);
        setTrailerOn(false);
        if (why !== "stopped") spent.add(hero.tile.id);
        // Played to its cap or end: the art fades back in, and the same clock pages on 600 ms later.
        if (why === "cap" || why === "ended") pager.trailer(1 - 600 / DWELL_MS);
      },
    });
  };
  // TV and the keyboard: Right from the last button pages forward, Left from the first back, the focus
  // staying on the same button; no wrap by key.
  const onKey = (e: KeyboardEvent) => {
    if (e.defaultPrevented || (e.key !== "ArrowRight" && e.key !== "ArrowLeft")) return;
    const el = document.activeElement as HTMLElement | null;
    const acts = el?.closest(".hero-page.on .acts");
    if (!el || !acts) return;
    const buttons = [...acts.querySelectorAll<HTMLElement>("[data-f]")];
    const forward = e.key === "ArrowRight";
    if (el !== (forward ? buttons.at(-1) : buttons[0])) return;
    const to = index() + (forward ? 1 : -1);
    if (to < 0 || to >= list().length) return;
    e.preventDefault();
    page(to);
  };
  /** The new page is live as soon as the signal is set: its same button takes the focus, else its first. */
  const refocus = (key: string) =>
    queueMicrotask(() => {
      const next = track.querySelector<HTMLElement>(`.hero-page.on [data-f="${key}"]`) ?? track.querySelector<HTMLElement>(".hero-page.on .acts [data-f]");
      next?.focus({ preventScroll: true });
    });

  const many = () => list().length > 1;
  const pageNearest = (e: MouseEvent & { currentTarget: HTMLElement }) => {
    const dots = [...e.currentTarget.querySelectorAll<HTMLElement>("button.dot")];
    const off = dots.map((d) => Math.abs(d.getBoundingClientRect().left + d.getBoundingClientRect().width / 2 - e.clientX));
    page(off.indexOf(Math.min(...off)));
  };
  // Every label Play says on these pages: its cell holds the widest of them too (an anime's "Resume S1:E1122").
  const labels = () => [...new Set(list().flatMap((h) => (h.play ? [h.play.label] : [])))];
  return (
    <section
      ref={section}
      class="hero"
      classList={{ closed: !list().length && !props.waiting, "trailer-on": shown() }}
      data-reveal-top
      // The first focus waits for the hero while it comes, and for the page's Play while the core reads it
      // (focus/dom.ts): a stopgap focus meanwhile would land on Details and be moved to Play when it came.
      data-focus-wait={(!list().length && props.waiting) || (!landed() && list()[index()]?.playPending && !list()[index()]?.play) ? "" : undefined}
      aria-label="Featured"
      aria-roledescription="carousel"
      onKeyDown={onKey}
      onPointerDown={() => (pressing = true)}
      onPointerUp={() => (pressing = false)}
      onPointerCancel={() => (pressing = false)}
      onPointerEnter={(e) => (hovered = e.pointerType === "mouse")}
      onPointerLeave={() => (hovered = false)}
    >
      <Show when={!list().length}>
        <Backdrop src={null} class="hero-bd" ramp={form() === "phone" ? "heroPhone" : "hero"} />
      </Show>
      <div
        ref={track}
        class="hero-track"
        data-hscroll="0"
        onScroll={depth}
        onPointerDown={() => (manual = true)}
        onWheel={(e) => Math.abs(e.deltaX) > Math.abs(e.deltaY) && (manual = true)}
      >
        <For each={list()}>
          {(h, i) => {
            const on = () => i() === index();
            const near = () => Math.abs(i() - index()) <= 1;
            return (
              <div
                class="hero-page"
                classList={{ on: on() }}
                data-page={i()}
                inert={!on() || undefined}
                aria-roledescription="slide"
                aria-label={`${i() + 1} of ${list().length}, ${h.tile.name}`}
              >
                <Backdrop src={near() ? h.tile.background : null} class="hero-bd" ramp={form() === "phone" ? "heroPhone" : "hero"} />
                <HeroWords hero={h} active={on()} labels={labels()} />
              </div>
            );
          }}
        </For>
      </div>
      <Show when={!list().length && props.waiting}>
        <div class="hero-body">
          <div class="hero-skel skel" aria-hidden="true" />
        </div>
      </Show>
      <div class="hero-trailer" aria-hidden="true">
        <div ref={box} class="hero-player" />
      </div>
      {/* The phone's large title and the profile button over the art, as the iPhone TV app. They never slide. */}
      <Show when={form() === "phone"}>
        <header class="ph-top">
          <h1 class="ph-title">Home</h1>
          <ProfileButton />
        </header>
      </Show>
      <TrailerMute />
      <Show when={many()}>
        {/* Phone: one 44 px tall strip, as iOS's page control; a tap between the 16 px pitched dots goes to the nearest.
            The dots stay buttons for the keyboard and a screen reader. */}
        <div class="hero-dots" classList={{ platter: form() === "desktop" }} onClick={(e) => e.target === e.currentTarget && form() === "phone" && pageNearest(e)}>
          <For each={list()}>
            {(h, i) => {
              const label = () => `Page ${i() + 1} of ${list().length}, ${h.tile.name}`;
              const set = (el: HTMLElement) => createEffect(() => i() === index() && (fill = el));
              return form() === "tv" ? (
                <span class="dot" classList={{ on: i() === index() }} aria-hidden="true">
                  <i ref={set} />
                </span>
              ) : (
                <button class="dot" classList={{ on: i() === index() }} aria-label={label()} aria-current={i() === index() ? "true" : undefined} onClick={() => page(i())}>
                  <i ref={set} />
                </button>
              );
            }}
          </For>
          {/* Desktop, the web site's form: a pause for the moving pages (WCAG 2.2.2). */}
          <Show when={form() === "desktop"}>
            <button class="dots-pause" aria-label={paused() ? "Resume" : "Pause"} onClick={() => setPaused(!paused())}>
              <Show when={paused()} fallback={<Pause aria-hidden />}>
                <Play aria-hidden />
              </Show>
            </button>
          </Show>
        </div>
      </Show>
      <p class="sr hero-said" role="status">
        {said()}
      </p>
    </section>
  );
}

/** One page's words: logo, genre line, meta with ratings, synopsis, the reason, Play and Details. Only
 * the active page carries focus keys. */
function HeroWords(props: { hero: Hero; active: boolean; labels: string[] }) {
  const h = () => props.hero;
  const key = (k: string) => (props.active ? k : undefined);
  return (
    <div class="hero-body">
      <Logo src={h().tile.logo} name={h().tile.name} class="hero-logo" level={form() === "phone" ? 2 : 1} />
      <div class="hero-lines">
        <p class="genre">{[kindLabel(h().tile.kind), ...h().genres].join(" · ")}</p>
        <p class="meta" ref={separators}>
          <Show when={h().tile.releaseInfo}>
            <span class="num yr">{h().tile.releaseInfo}</span>
          </Show>
          <Show when={h().runtime}>
            <b aria-hidden="true" class="yr">·</b>
            <span class="num yr">{h().runtime}</span>
          </Show>
          <Show when={(h().tile.releaseInfo || h().runtime) && ratingParts(h().tile.ratings).length}>
            <b aria-hidden="true" class="yr">·</b>
          </Show>
          <Ratings ratings={h().tile.ratings} sep={() => <b aria-hidden="true">·</b>} />
        </p>
      </div>
      <Show when={h().description}>
        <p class="hero-syn">{h().description}</p>
      </Show>
      <p class="reason">{h().reason}</p>
      {/* One stack: Up from the first row lands on its first button, Play (`data-fentry`). */}
      <div class="acts" data-fgroup={key("hero")} data-fentry={props.active ? "" : undefined}>
        {/* The core's target, as the title page's Play: "Play S1:E1" for a series, "Resume" part way in. The
            player resumes from the core's stored position. */}
        <Show
          when={h().play}
          fallback={
            // Play's place while the core reads the page's episodes (`playPending`): Details never moves when it
            // lands (review r4). Hidden, out of the focus order and of the accessibility tree. Meanwhile no button
            // is the first focus: it waits for Play, so it lands once and never moves under the viewer's thumb
            // (focus/dom.ts lands it on the first button after 4 s if Play is still out).
            <Show when={h().playPending}>
              <span class="btn primary slot" aria-hidden="true">
                <Play aria-hidden class="fill" />
                <PlayWords label={h().tile.kind === "movie" ? "Play" : "Play S1:E1"} labels={props.labels} />
              </span>
            </Show>
          }
        >
          {(play) => (
            <A
              class="btn primary"
              href={playPath(h().tile.kind, h().tile.id, play().videoId)}
              data-f={key("hero:play")}
              aria-label={`${play().label}, ${h().tile.name}`}
              data-autofocus={props.active ? "" : undefined}
            >
              <Play aria-hidden class="fill" />
              <PlayWords label={play().label} labels={props.labels} />
            </A>
          )}
        </Show>
        {/* Details: a pill with its word on TV, as tvOS; the round button off the TV. */}
        <Show
          when={form() === "tv"}
          fallback={
            <A
              class="rbtn"
              href={titlePath(h().tile.kind, h().tile.id)}
              data-f={key("hero:details")}
              aria-label={`Details for ${h().tile.name}`}
              data-autofocus={props.active && !h().play && !h().playPending ? "" : undefined}
            >
              <span class="ring">
                <Info aria-hidden />
              </span>
              <span class="lbl">Details</span>
            </A>
          }
        >
          <A
            class="btn"
            href={titlePath(h().tile.kind, h().tile.id)}
            data-f={key("hero:details")}
            aria-label={`Details for ${h().tile.name}`}
            data-autofocus={props.active && !h().play && !h().playPending ? "" : undefined}
          >
            <Info aria-hidden />
            Details
          </A>
        </Show>
      </div>
    </div>
  );
}

/** Play's words in a cell as wide as the longest it can say (home.css `.play-words`): the widest labels the core
 * gives and every label on the carousel's pages (review r5, R5-3), so Details never moves whatever lands (the lead,
 * 2026-10-26); the phone's Play is a fixed width already. The other labels are drawn by CSS from `data-l`, so the
 * button's text and name stay its own. */
function PlayWords(props: { label: string; labels: string[] }) {
  return (
    <span class="play-words">
      <span>{props.label}</span>
      <For each={props.labels}>{(l) => <span class="pw-other" data-l={l} aria-hidden="true" />}</For>
    </span>
  );
}
