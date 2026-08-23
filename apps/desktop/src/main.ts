import App from "./App.svelte";
import "./styles.scss";
import { mount } from "svelte";

declare const __AIPASS_RELEASE_BUILD__: boolean;

// The native browser context menu exposes reload and, when available, the
// inspector. Keep it available during development, but remove it from the
// packaged application. Bits UI context menus handle their own event first
// and remain available because this listener runs during bubbling.
if (__AIPASS_RELEASE_BUILD__) {
  document.addEventListener("contextmenu", (event) => event.preventDefault());
}

const app = mount(App, {
  target: document.getElementById("app") as HTMLElement
});

export default app;
