import { A } from "@solidjs/router";
import Monitor from "lucide-solid/icons/monitor";
import Smartphone from "lucide-solid/icons/smartphone";
import Tv from "lucide-solid/icons/tv";
import Wrench from "lucide-solid/icons/wrench";
import { For } from "solid-js";
import { chooseForm, type Form, form } from "../form";
import { Stub } from "./stub";

const forms: { form: Form; label: string; icon: typeof Tv }[] = [
  { form: "tv", label: "TV", icon: Tv },
  { form: "desktop", label: "Desktop", icon: Monitor },
  { form: "phone", label: "Phone", icon: Smartphone },
];

export default function SettingsScreen() {
  return (
    <Stub title="Settings" text="Profiles, playback and addon management arrive with their core views. These work today.">
      <section>
        <h2 class="kicker">Layout for this device</h2>
        <div class="acts" role="radiogroup" aria-label="Layout">
          <For each={forms}>
            {(f, i) => (
              <button
                class="btn"
                classList={{ primary: form() === f.form }}
                role="radio"
                aria-checked={form() === f.form}
                data-f={`form:${f.form}`}
                data-autofocus={i() === 0 ? "" : undefined}
                onClick={() => chooseForm(f.form)}
              >
                <f.icon aria-hidden />
                {f.label}
              </button>
            )}
          </For>
          <button class="btn" data-f="form:auto" onClick={() => chooseForm(null)}>
            Detect automatically
          </button>
        </div>
      </section>
      <section>
        <h2 class="kicker">Sources</h2>
        <div class="acts">
          <A class="btn" href="/welcome" data-f="settings:setup">
            <Wrench aria-hidden />
            Add an addon and run the setup check
          </A>
        </div>
      </section>
    </Stub>
  );
}
