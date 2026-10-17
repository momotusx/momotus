import "@fontsource-variable/inter/index.css";
import "@fontsource/ibm-plex-mono/latin-400.css";
import "./theme/tokens.css";
import "./theme/app.css";
import { render } from "solid-js/web";
import { App, markBack } from "./app";
import { startFitting } from "./form";

// Registered before the router's own listener so a browser Back is known
// before the new screen asks whether to restore focus.
window.addEventListener("popstate", markBack);
startFitting();
render(() => <App />, document.getElementById("root") as HTMLElement);
