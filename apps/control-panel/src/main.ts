import { mount } from "svelte";
import App from "./App.svelte";
import "./style.scss";

mount(App, { target: document.getElementById("app")! });
