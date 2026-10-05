/// <reference path="./assets.d.ts" />
import type { ProviderIconName } from "@aipass/schemas";

import alibaba from "./assets/provider-icons/alibaba.svg?inline";
import anthropic from "./assets/provider-icons/anthropic.svg?inline";
import anyscale from "./assets/provider-icons/anyscale.svg?inline";
import cloudflare from "./assets/provider-icons/cloudflare.svg?inline";
import cohere from "./assets/provider-icons/cohere.svg?inline";
import deepseek from "./assets/provider-icons/deepseek.svg?inline";
import fireworks from "./assets/provider-icons/fireworks.svg?inline";
import google from "./assets/provider-icons/google.svg?inline";
import groq from "./assets/provider-icons/groq.svg?inline";
import koboldcpp from "./assets/provider-icons/koboldcpp.svg?inline";
import lmstudio from "./assets/provider-icons/lmstudio.svg?inline";
import localai from "./assets/provider-icons/localai.svg?inline";
import mistral from "./assets/provider-icons/mistral.svg?inline";
import moonshot from "./assets/provider-icons/moonshot.svg?inline";
import octoai from "./assets/provider-icons/octoai.svg?inline";
import ollama from "./assets/provider-icons/ollama.svg?inline";
import openai from "./assets/provider-icons/openai.svg?inline";
import openrouter from "./assets/provider-icons/openrouter.svg?inline";
import perplexity from "./assets/provider-icons/perplexity.svg?inline";
import replicate from "./assets/provider-icons/replicate.svg?inline";
import siliconflow from "./assets/provider-icons/siliconflow.svg?inline";
import stepfun from "./assets/provider-icons/stepfun.svg?inline";
import textgen from "./assets/provider-icons/textgen.svg?inline";
import together from "./assets/provider-icons/together.svg?inline";
import vllm from "./assets/provider-icons/vllm.svg?inline";
import xai from "./assets/provider-icons/xai.svg?inline";

// Bundle URLs into every surface; the control panel serves only embedded assets.
export const builtInProviderIcons: Record<ProviderIconName, string> = {
  alibaba,
  anthropic,
  anyscale,
  cloudflare,
  cohere,
  deepseek,
  fireworks,
  google,
  groq,
  koboldcpp,
  lmstudio,
  localai,
  mistral,
  moonshot,
  octoai,
  ollama,
  openai,
  openrouter,
  perplexity,
  replicate,
  siliconflow,
  stepfun,
  textgen,
  together,
  vllm,
  xai,
};

// SVG currentColor does not inherit through <img>; masks retain the UI tone.
export const monochromeProviderIcons = new Set<ProviderIconName>(["openai", "anthropic", "openrouter", "ollama"]);
