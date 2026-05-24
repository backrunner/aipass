import Popup from "./Popup.svelte";
import "./popup.scss";
import { mount } from "svelte";

mount(Popup, {
  target: document.getElementById("app") as HTMLElement
});
