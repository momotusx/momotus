/** Calendar: episodes of the library's series from a week back to a month
 * ahead, by air date. The core works from the app's local date, so the screen
 * sends `setToday` at local midnight and whenever the app comes back to the
 * foreground. Selecting an episode opens its title on that season. */

import { A } from "@solidjs/router";
import Check from "lucide-solid/icons/check";
import LoaderCircle from "lucide-solid/icons/loader-circle";
import { createEffect, createSignal, For, onCleanup, onMount, Show } from "solid-js";
import { liveView } from "../core/client";
import type { CalendarDay } from "../core/generated/CalendarDay";
import type { CalendarItem } from "../core/generated/CalendarItem";
import type { CalendarSnapshot } from "../core/generated/CalendarSnapshot";
import { form } from "../form";
import { episodeLabel, titlePath } from "../lib/format";
import "./browse.css";
import "./calendar.css";

const pad = (n: number) => String(n).padStart(2, "0");
/** The local calendar date, never shifted to UTC. */
export const localDay = (d = new Date()) => `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())}`;
const asDate = (day: string) => {
  const [y, m, d] = day.split("-").map(Number);
  return new Date(y ?? 1970, (m ?? 1) - 1, d ?? 1);
};
const daysBetween = (from: string, to: string) => Math.round((asDate(to).getTime() - asDate(from).getTime()) / 86_400_000);

/** "Today", "Tomorrow", "Yesterday", else the weekday; the date goes beside it. */
export function dayName(day: string, today: string): string {
  const diff = daysBetween(today, day);
  if (diff === 0) return "Today";
  if (diff === 1) return "Tomorrow";
  if (diff === -1) return "Yesterday";
  return asDate(day).toLocaleDateString("en", { weekday: "long" });
}
const dayDate = (day: string) => asDate(day).toLocaleDateString("en", { month: "long", day: "numeric" });
const shortDate = (day: string) => asDate(day).toLocaleDateString("en", { month: "short", day: "numeric" });

const itemHref = (it: CalendarItem) => `${titlePath("series", it.itemId)}${it.season !== null ? `?season=${it.season}` : ""}`;
const itemKey = (it: CalendarItem) => `cal:${it.videoId}`;

export default function CalendarScreen() {
  let sent = localDay();
  const view = liveView<CalendarSnapshot>("calendar", { today: sent });
  const s = () => view.state.data;
  const setToday = () => {
    sent = localDay();
    view.command({ type: "setToday", today: sent }).catch(() => undefined);
  };
  // Local midnight, then every midnight after; and back from the background.
  onMount(() => {
    let timer: ReturnType<typeof setTimeout>;
    const arm = () => {
      const now = new Date();
      const next = new Date(now.getFullYear(), now.getMonth(), now.getDate() + 1, 0, 0, 1);
      timer = setTimeout(() => {
        setToday();
        arm();
      }, next.getTime() - now.getTime());
    };
    arm();
    const onVisible = () => {
      if (!document.hidden) setToday();
    };
    document.addEventListener("visibilitychange", onVisible);
    onCleanup(() => {
      clearTimeout(timer);
      document.removeEventListener("visibilitychange", onVisible);
    });
  });

  // The first day on or after today sits at the top with its first episode
  // focused. Series keep arriving while the schedule loads and add days above
  // it, so it is placed again on every update until the viewer acts.
  const target = () => s()?.days.find((d) => d.date >= (s()?.today ?? sent)) ?? null;
  const [acted, setActed] = createSignal(false);
  onMount(() => {
    const stop = () => setActed(true);
    const events = ["keydown", "pointerdown", "wheel", "touchstart"] as const;
    for (const e of events) window.addEventListener(e, stop, { capture: true, passive: true });
    onCleanup(() => {
      for (const e of events) window.removeEventListener(e, stop, { capture: true });
    });
  });
  createEffect(() => {
    const day = target()?.date;
    void s()?.days.length;
    if (!day || acted()) return;
    queueMicrotask(() => {
      const section = document.querySelector<HTMLElement>(`[data-day="${day}"]`);
      section?.scrollIntoView({ block: "start" });
      const first = section?.querySelector<HTMLElement>("[data-f]");
      if (first && form() !== "phone") first.focus({ preventScroll: true });
    });
  });

  const span = () => {
    const t = s()?.today ?? sent;
    const from = asDate(t);
    from.setDate(from.getDate() - 7);
    const to = asDate(t);
    to.setDate(to.getDate() + 30);
    return `${shortDate(localDay(from))} to ${shortDate(localDay(to))}`;
  };
  const empty = () => !!s() && s()?.days.length === 0 && s()?.pending === 0;

  return (
    <main class="browse page cal" data-vscroll>
      <header class="cal-head">
        <h1 data-eyebrow="Coming up">Calendar</h1>
        <p class="lede">New episodes of the series in your library, {span()}.</p>
        <Show when={(s()?.pending ?? 0) > 0}>
          <p class="st warn" role="status">
            <LoaderCircle aria-hidden class="spin" />
            Loading the schedule of {s()?.pending} series
          </p>
        </Show>
        <Show when={view.state.status.state === "failed"}>
          <p class="st bad" role="status">
            <i class="dot" />
            {(view.state.status as { reason: string }).reason}
          </p>
        </Show>
        <Show when={s()?.failed.length}>
          <p class="st bad cal-note">
            <i class="dot" />
            Could not load {s()?.failed.map((f) => (f.reason ? `${f.name} (${f.reason})` : f.name)).join(", ")}.
          </p>
        </Show>
        <Show when={s()?.noSchedule.length}>
          <p class="cal-note">No air dates known for {s()?.noSchedule.map((f) => f.name).join(", ")}.</p>
        </Show>
      </header>

      <Show when={empty()}>
        <p class="note">
          {s()?.series ? "Nothing airs in these weeks for the series in your library." : "No series in your library yet. Series you add show their episodes here."}{" "}
          <A href="/discover">Find series to follow</A>.
        </p>
      </Show>

      <For each={s()?.days ?? []}>{(day) => <Day day={day} today={s()?.today ?? sent} first={day.date === target()?.date} />}</For>
    </main>
  );
}

/** The episode still, else the series poster, else the series name. */
function Art(props: { item: CalendarItem }) {
  const sources = () => [props.item.still, props.item.poster].filter((u): u is string => !!u);
  const [failed, setFailed] = createSignal(0);
  const src = () => sources()[failed()] ?? null;
  return (
    <span class="cal-art">
      <Show when={src()} keyed fallback={<span class="cal-name">{props.item.name}</span>}>
        {(url) => (
          <img
            class="fade"
            classList={{ poster: url !== props.item.still }}
            src={url}
            alt=""
            width={320}
            height={180}
            decoding="async"
            onLoad={(e) => e.currentTarget.classList.add("in")}
            onError={() => setFailed(failed() + 1)}
          />
        )}
      </Show>
    </span>
  );
}

function Day(props: { day: CalendarDay; today: string; first: boolean }) {
  const past = () => props.day.date < props.today;
  return (
    <section class="cal-day" classList={{ past: past(), today: props.day.date === props.today }} data-day={props.day.date} data-row aria-label={`${dayName(props.day.date, props.today)}, ${dayDate(props.day.date)}`}>
      <h2>
        {dayName(props.day.date, props.today)}
        <small>{dayDate(props.day.date)}</small>
      </h2>
      <div class="cal-track" data-hscroll="80" data-fgroup={`cal:${props.day.date}`}>
        <For each={props.day.items}>
          {(it, i) => (
            <A class="cal-card" href={itemHref(it)} data-f={itemKey(it)} data-autofocus={props.first && i() === 0 ? "" : undefined}>
              <Art item={it} />
              <span class="cal-text">
                <b>{it.name}</b>
                <span class="cal-ep">
                  <Show when={it.season !== null && it.episode !== null}>
                    <span class="num">{episodeLabel(it.season ?? 0, it.episode ?? 0)}</span>
                  </Show>
                  {it.title}
                </span>
                <Show when={it.watched}>
                  <span class="cal-seen">
                    <Check aria-hidden />
                    Watched
                  </span>
                </Show>
              </span>
            </A>
          )}
        </For>
      </div>
    </section>
  );
}
