import { A } from "@solidjs/router";
import CalendarDays from "lucide-solid/icons/calendar-days";
import ChevronRight from "lucide-solid/icons/chevron-right";
import Compass from "lucide-solid/icons/compass";
import RefreshCw from "lucide-solid/icons/refresh-cw";
import { createSignal, Match, Show, Switch } from "solid-js";
import { Chips, Grid, SkeletonGrid } from "../components/grid";
import { Why } from "../components/why";
import { libraryView } from "../core/client";
import { onProfileReset } from "../core/profiles";
import { restoreSoon } from "../focus/dom";
import { form } from "../form";
import type { LibrarySort } from "../core/generated/LibrarySort";
import "./browse.css";

const sorts: { value: LibrarySort; label: string }[] = [
  { value: "recent", label: "Last watched" },
  { value: "added", label: "Recently added" },
  { value: "name", label: "Name" },
];

// Where the page was scrolled to: the view is kept, so Back lands here too.
const pageScroll = { top: 0 };
onProfileReset(() => {
  pageScroll.top = 0;
});

export default function Library() {
  // Try again on a library that could not open: a new view, as the page anew.
  const [attempt, setAttempt] = createSignal(0);
  // The pressed button goes with the old page: the focus waits for the new one's first control, not the lit tab.
  const retry = () => {
    setAttempt((n) => n + 1);
    restoreSoon(false);
  };
  return (
    <Show when={attempt() + 1} keyed>
      <LibraryPage retry={retry} />
    </Show>
  );
}

function LibraryPage(props: { retry: () => void }) {
  const view = libraryView();
  const attach = (el: HTMLElement) => {
    if (pageScroll.top) queueMicrotask(() => el.scrollTo({ top: pageScroll.top }));
  };
  const l = () => view.state.data;
  const total = () => l()?.kinds.reduce((n, k) => n + k.count, 0) ?? 0;

  return (
    <main class="browse page" data-vscroll ref={attach} onScroll={(e) => (pageScroll.top = e.currentTarget.scrollTop)}>
      <h1>Library</h1>
      {/* The phone's tab bar has five tabs: Calendar is a row here, and Library stays lit on it. */}
      <Show when={form() === "phone"}>
        <A class="cal-row" href="/calendar">
          <CalendarDays aria-hidden />
          <span>Calendar</span>
          <ChevronRight aria-hidden class="chev" />
        </A>
      </Show>
      <Switch fallback={<SkeletonGrid />}>
        <Match when={view.state.status.state === "failed"}>
          <section class="problem">
            <h2>Your library could not open</h2>
            <Why reason={(view.state.status as { reason: string }).reason} />
            <button class="btn primary" data-f="library:retry" data-autofocus onClick={() => props.retry()}>
              <RefreshCw aria-hidden />
              Try again
            </button>
          </section>
        </Match>
        <Match when={l() && total() === 0}>
          <section class="problem">
            <h2>Your library is empty</h2>
            <p>Add titles with Add to Library on a title page, or import your Stremio library from Settings.</p>
            <A class="btn primary" href="/discover" data-f="library:discover" data-autofocus>
              <Compass aria-hidden />
              Discover
            </A>
          </section>
        </Match>
        <Match when={l()}>
          {(s) => (
            <>
              <div class="filters">
                <Chips
                  label="Show"
                  group="lib-kinds"
                  items={[{ value: null as string | null, label: "All", note: String(total()) }, ...s().kinds.map((k) => ({ value: k.kind as string | null, label: k.label, note: String(k.count) }))]}
                  selected={s().kind}
                  onSelect={(kind) => void view.command({ type: "setFilter", kind })}
                  autofocus
                />
                <Chips label="Sort" group="lib-sort" items={sorts} selected={s().sort} onSelect={(sort) => void view.command({ type: "setSort", sort })} />
              </div>
              <Show when={s().kind === null || s().items.length} fallback={
                <section class="problem">
                  <h2>Nothing of this type in your library</h2>
                  <button class="btn primary" data-f="library:all" onClick={() => void view.command({ type: "setFilter", kind: null })}>
                    Show all
                  </button>
                </section>
              }>
                <Grid items={s().items} scope="library" />
              </Show>
            </>
          )}
        </Match>
      </Switch>
    </main>
  );
}
