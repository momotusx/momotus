/** App updates on webOS (plans/auto-update.md section 5): About's row (its state, at most one action) and Home's quiet
 * line while an update waits for Install. Nothing shows where the service has no updates (state null). */

import { A } from "@solidjs/router";
import Download from "lucide-solid/icons/download";
import RefreshCw from "lucide-solid/icons/refresh-cw";
import { Show } from "solid-js";
import { fillUpdate, updateRow, updateState, updateWords } from "../core/webos-update";

export function UpdateRow() {
  return (
    <Show when={updateState()}>
      {(s) => {
        const row = () => updateRow(s());
        return (
          <div class="sgroup">
            <Show
              when={row().action}
              fallback={
                <p class="note" role="status" data-f="settings:update-line">
                  {row().line}
                </p>
              }
            >
              {(a) => (
                <button class="drow" data-f="settings:update" onClick={a().run}>
                  <span class="dlogo" aria-hidden="true">
                    {a().install ? <Download /> : <RefreshCw />}
                  </span>
                  <span class="dtext">
                    <b>{a().label}</b>
                    <Show when={row().line}>
                      <small role="status">{row().line}</small>
                    </Show>
                  </span>
                </button>
              )}
            </Show>
          </div>
        );
      }}
    </Show>
  );
}

/** Home's one quiet line while an update waits for Install: it opens About, where Install is. */
export function UpdateNote() {
  return (
    <Show when={updateState()?.kind === "ready" && updateState()}>
      {(s) => (
        <A class="update-note" href="/settings/about" data-f="home:update">
          <Download aria-hidden />
          <span>{fillUpdate(updateWords.home, s())}</span>
        </A>
      )}
    </Show>
  );
}
