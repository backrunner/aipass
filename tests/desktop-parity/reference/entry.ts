import { mount } from "svelte";
import App from "../../../apps/desktop/src/App.svelte";
import UnlockTransition from "../../../apps/desktop/src/lib/components/auth/UnlockTransition.svelte";
import DynamicReference from "./DynamicReference.svelte";
import SubscriptionReference from "./SubscriptionReference.svelte";
import "../../../apps/desktop/src/styles.scss";
window.__TAURI_INTERNALS__ ||= {};
document.documentElement.dataset.platform = "darwin";
const params = new URLSearchParams(location.search);
if ((params.get("scenario") ?? "").startsWith("subscription-")) {
  mount(SubscriptionReference, { target: document.getElementById("app")! });
} else if (["unlock-motion", "lock-motion"].includes(params.get("scenario") ?? "")) {
  document.documentElement.dataset.theme = params.get("theme") ?? "light";
  mount(UnlockTransition, { target: document.getElementById("app")!,
    props: { direction: params.get("scenario") === "lock-motion" ? "down" : "up" } });
} else if ((params.get("scenario") ?? "").startsWith("dynamic-") || (params.get("scenario") ?? "").startsWith("toast")) {
  mount(DynamicReference, { target: document.getElementById("app")! });
} else {
  mount(App, { target: document.getElementById("app")! });
}
