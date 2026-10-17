import History from "lucide-solid/icons/rotate-ccw-clock";
import SearchIcon from "lucide-solid/icons/search";
import { createSignal, For, Match, Show, Switch } from "solid-js";
import { Row } from "../components/row";
import { liveView } from "../core/client";
import type { SearchSnapshot } from "../core/generated/SearchSnapshot";
import "./browse.css";
import { onProfileReset } from "../core/profiles";

// The last query survives leaving the screen, so Back lands on the results.
let lastQuery = "";
// Recent searches belong to the profile that typed them.
onProfileReset(() => {
  lastQuery = "";
});

export default function Search() {
  const view = liveView<SearchSnapshot>("search");
  const [text, setText] = createSignal(lastQuery);
  const [page, setPage] = createSignal<HTMLElement | null>(null);
  const s = () => view.state.data;
  const send = (q: string) => {
    lastQuery = q;
    setText(q);
    void view.command({ type: "setQuery", query: q });
  };
  if (lastQuery) queueMicrotask(() => send(lastQuery));
  const searching = () => text().trim().length > 0;
  const loading = () => s()?.rows.some((r) => r.state.state === "loading" || r.state.state === "slow");

  return (
    <main class="browse page search" data-vscroll ref={setPage}>
      <label class="field" data-row>
        <SearchIcon aria-hidden />
        <span class="sr">Search</span>
        <input
          type="search"
          value={text()}
          placeholder="Search titles"
          autocomplete="off"
          spellcheck={false}
          enterkeyhint="search"
          data-f="search:input"
          data-autofocus
          onInput={(e) => send(e.currentTarget.value)}
        />
      </label>
      <Switch>
        <Match when={view.state.status.state === "failed"}>
          <section class="problem">
            <h2>Search could not open</h2>
            <p>{(view.state.status as { reason: string }).reason}</p>
          </section>
        </Match>
        <Match when={!searching()}>
          <Show when={s()?.recent.length} fallback={<p class="note">Type a title. Results come from every addon with a search catalog.</p>}>
            <section class="recent">
              <h2 class="kicker">Recent searches</h2>
              <div class="chips-track wrap" data-fgroup="recent">
                <For each={s()?.recent ?? []}>
                  {(q) => (
                    <button class="chip" data-f={`recent:${q}`} onClick={() => send(q)}>
                      <History aria-hidden />
                      {q}
                    </button>
                  )}
                </For>
              </div>
            </section>
          </Show>
        </Match>
        <Match when={s() && s()?.query === text() && !loading() && s()?.rows.length === 0}>
          <p class="note">No matches for "{text()}" in your catalogs.</p>
        </Match>
        <Match when={true}>
          <div class="results">
            <For each={s()?.rows ?? []}>
              {(row) => <Row row={row} autofocus={false} root={page} scope="search" onOpen={() => void view.command({ type: "remember" })} />}
            </For>
          </div>
        </Match>
      </Switch>
    </main>
  );
}
