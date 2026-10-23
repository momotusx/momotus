/** Why nothing can play: its actions in the shared table's order
 * (plans/play-every-version.md, lib/words.ts emptyWords), the first primary
 * and focused. The page adds Back after them, and Choose a version (the
 * player's and the title's) through `choose`. */

import { A } from "@solidjs/router";
import Play from "lucide-solid/icons/play";
import RefreshCw from "lucide-solid/icons/refresh-cw";
import Settings from "lucide-solid/icons/settings";
import { For, type JSX, Match, Switch } from "solid-js";
import type { EmptyVersions } from "../core/generated/EmptyVersions";
import { emptyWords } from "../lib/words";

export function EmptyActs(props: { empty: EmptyVersions; at: string; big?: boolean; onVersions?: boolean; retry: () => void; play: (key: string) => void; choose?: JSX.Element }) {
  const cls = (i: number) => ({ btn: true, big: !!props.big, primary: i === 0 });
  const first = (i: number) => (i === 0 ? "" : undefined);
  return (
    <For each={emptyWords(props.empty, props.onVersions).steps}>
      {(s, i) => (
        <Switch>
          <Match when={s.step === "settingsAddons" || s.step === "settingsAccounts"}>
            <A classList={cls(i())} href={s.step === "settingsAddons" ? "/addons" : "/debrid"} data-f={`${props.at}:settings`} data-autofocus={first(i())}>
              <Settings aria-hidden />
              {s.text}
            </A>
          </Match>
          <Match when={s.step === "playFromDevice" && props.empty.key}>
            {(key) => (
              <button classList={cls(i())} data-f={`${props.at}:device`} data-autofocus={first(i())} onClick={() => props.play(key())}>
                <Play aria-hidden class="fill" />
                {s.text}
              </button>
            )}
          </Match>
          <Match when={s.step === "tryAgain"}>
            <button classList={cls(i())} data-f={`${props.at}:retry`} data-autofocus={first(i())} onClick={() => props.retry()}>
              <RefreshCw aria-hidden />
              {s.text}
            </button>
          </Match>
          <Match when={s.step === "chooseVersion"}>{props.choose}</Match>
        </Switch>
      )}
    </For>
  );
}
