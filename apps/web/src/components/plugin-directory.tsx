/** Find addons' Nuvio plugins (R8): the repositories nuvioplugins.com lists, in its order, each a
 * card with its name, provider count, kinds and languages, searched on the device across names,
 * providers and languages. Add reads the repository and asks on R7's sheet first; one the profile
 * has says Installed and is removed in Settings > Sources > Plugins. Nothing shows for a kids
 * profile or a build without plugins (the core lists none). */

import Check from "lucide-solid/icons/check";
import Plus from "lucide-solid/icons/plus";
import { createMemo, createResource, createSignal, For, Show } from "solid-js";
import { op, reasonOf } from "../core/client";
import type { ListedRepo } from "../core/generated/ListedRepo";
import type { PluginDirectory } from "../core/generated/PluginDirectory";
import type { PluginRepoPreview } from "../core/generated/PluginRepoPreview";
import { form } from "../form";
import { addsProviders, directoryLine, fill, matchesRepo, pluginWords as w } from "../lib/plugins";
import { InstallSheet } from "./install-sheet";

export function PluginDirectorySection() {
  const [listed, { refetch }] = createResource(() => op<PluginDirectory>("pluginDirectory").catch(() => null));
  const [query, setQuery] = createSignal("");
  const [asking, setAsking] = createSignal<PluginRepoPreview | null>(null);
  const [said, setSaid] = createSignal<{ url: string; tone: string; text: string } | null>(null);
  const repos = () => listed()?.repos ?? [];
  const shown = createMemo(() => repos().filter((r) => matchesRepo(r, query())));
  const tv = () => form() === "tv";
  const ask = async (r: ListedRepo) => {
    setSaid({ url: r.url, tone: "warn live", text: w.checking });
    try {
      setAsking(await op<PluginRepoPreview>("previewPluginRepo", { url: r.url }));
      setSaid(null);
    } catch (err) {
      setSaid({ url: r.url, tone: "bad", text: fill(w.couldNotAdd, { reason: reasonOf(err) }) });
    }
  };
  const add = async (p: PluginRepoPreview) => {
    setAsking(null);
    setSaid({ url: p.url, tone: "warn live", text: w.adding });
    try {
      await op("addPluginRepo", { url: p.url });
      setSaid({ url: p.url, tone: "ok", text: fill(w.added, { name: p.name }) });
      void refetch();
    } catch (err) {
      setSaid({ url: p.url, tone: "bad", text: fill(w.couldNotAdd, { reason: reasonOf(err) }) });
    }
  };
  return (
    <Show when={repos().length > 0}>
      <section class="fa-plugins" aria-labelledby="fa-plugins-h">
        <h2 id="fa-plugins-h">{w.directoryTitle}</h2>
        <p class="fa-plugins-from">{fill(w.directoryFrom, { source: listed()?.source ?? "" })}</p>
        <Show when={!tv()}>
          <input
            class="fa-search"
            type="search"
            data-f="fa-plugins:search"
            aria-label={w.directorySearch}
            placeholder={w.directorySearch}
            value={query()}
            onInput={(e) => setQuery(e.currentTarget.value)}
          />
        </Show>
        <Show when={shown().length > 0} fallback={<p class="note">{w.directoryNoMatch}</p>}>
          <ul class="fa-grid">
            <For each={shown()}>
              {(r, i) => (
                <li class="fa-cell">
                  <div class="fa-pcard" data-fgroup={`fa-plugin:${i()}`}>
                    <div class="fa-mark" aria-hidden="true">
                      {r.name.charAt(0).toUpperCase()}
                    </div>
                    <div class="fa-text">
                      <h2 dir="auto">{r.name}</h2>
                      <p class="fa-host">{fill(w.directoryFrom, { source: listed()?.source ?? "" })}</p>
                      <p class="fa-provides">{directoryLine(r)}</p>
                      <Show when={r.description}>
                        <p class="fa-desc" dir="auto">
                          {r.description}
                        </p>
                      </Show>
                      <Show when={said()?.url === r.url ? said() : null}>
                        {(s) => (
                          <p class={`st ${s().tone}`} role="status">
                            <i class="dot" />
                            {s().text}
                          </p>
                        )}
                      </Show>
                    </div>
                    <div class="fa-acts">
                      <Show
                        when={!r.installed}
                        fallback={
                          <span class="fa-state on">
                            <Check aria-hidden />
                            {w.installed}
                          </span>
                        }
                      >
                        <button class="btn" data-f={`fa-plugin:${i()}:act`} aria-label={`${w.add} ${r.name}`} onClick={() => void ask(r)}>
                          <Plus aria-hidden />
                          {w.add}
                        </button>
                      </Show>
                    </div>
                  </div>
                </li>
              )}
            </For>
          </ul>
        </Show>
        <Show when={asking()}>
          {(p) => (
            <InstallSheet
              addon={{ name: p().name, host: p().host, provides: [], types: [] }}
              words={{
                title: fill(w.sheetTitle, { name: p().name }),
                from: fill(w.sheetFrom, { host: p().host }),
                adds: addsProviders(p().providers),
                note: w.sheetNote,
                install: w.add,
              }}
              onCancel={() => setAsking(null)}
              onInstall={() => void add(p())}
            />
          )}
        </Show>
      </section>
    </Show>
  );
}
