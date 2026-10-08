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
import lmstudio from "./assets/provider-icons/lmstudio.svg?inline";
import localai from "./assets/provider-icons/localai.svg?inline";
import mistral from "./assets/provider-icons/mistral.svg?inline";
import moonshot from "./assets/provider-icons/moonshot.svg?inline";
import ollama from "./assets/provider-icons/ollama.svg?inline";
import openai from "./assets/provider-icons/openai.svg?inline";
import openrouter from "./assets/provider-icons/openrouter.svg?inline";
import perplexity from "./assets/provider-icons/perplexity.svg?inline";
import replicate from "./assets/provider-icons/replicate.svg?inline";
import siliconflow from "./assets/provider-icons/siliconflow.svg?inline";
import stepfun from "./assets/provider-icons/stepfun.svg?inline";
import together from "./assets/provider-icons/together.svg?inline";
import vllm from "./assets/provider-icons/vllm.svg?inline";
import xai from "./assets/provider-icons/xai.svg?inline";

import claude from "./assets/provider-icons/claude.svg?inline";
import codex from "./assets/provider-icons/codex.svg?inline";
import gemini from "./assets/provider-icons/gemini.svg?inline";
import copilot from "./assets/provider-icons/copilot.svg?inline";
import cursor from "./assets/provider-icons/cursor.svg?inline";
import kiro from "./assets/provider-icons/kiro.svg?inline";
import qoder from "./assets/provider-icons/qoder.svg?inline";
import devin from "./assets/provider-icons/devin.svg?inline";
import zai from "./assets/provider-icons/zai.svg?inline";
import mimo from "./assets/provider-icons/mimo.svg?inline";
import commandcode from "./assets/provider-icons/commandcode.svg?inline";
import azure from "./assets/provider-icons/azure.svg?inline";
import bedrock from "./assets/provider-icons/bedrock.svg?inline";
import nvidia from "./assets/provider-icons/nvidia.svg?inline";
import novita from "./assets/provider-icons/novita.svg?inline";
import minimax from "./assets/provider-icons/minimax.svg?inline";
import huggingface from "./assets/provider-icons/huggingface.svg?inline";
import cerebras from "./assets/provider-icons/cerebras.svg?inline";
import volcengine from "./assets/provider-icons/volcengine.svg?inline";
import qwen from "./assets/provider-icons/qwen.svg?inline";
import zhipu from "./assets/provider-icons/zhipu.svg?inline";
import newapi from "./assets/provider-icons/newapi.svg?inline";
import factory from "./assets/provider-icons/factory.svg?inline";
import workbuddy from "./assets/provider-icons/workbuddy.svg?inline";
import zed from "./assets/provider-icons/zed.svg?inline";

import grok from "./assets/provider-icons/grok.svg?inline";

// Bundle URLs into every surface; the control panel serves only embedded assets.
export const builtInProviderIcons: Record<ProviderIconName, string> = {
  claude,
  codex,
  gemini,
  copilot,
  cursor,
  kiro,
  qoder,
  devin,
  zai,
  mimo,
  commandcode,
  azure,
  bedrock,
  nvidia,
  novita,
  minimax,
  huggingface,
  cerebras,
  volcengine,
  qwen,
  zhipu,
  newapi,
  factory,
  workbuddy,
  zed,
  grok,
  alibaba,
  anthropic,
  anyscale,
  cloudflare,
  cohere,
  deepseek,
  fireworks,
  google,
  groq,
  lmstudio,
  localai,
  mistral,
  moonshot,
  ollama,
  openai,
  openrouter,
  perplexity,
  replicate,
  siliconflow,
  stepfun,
  together,
  vllm,
  xai,
};

// SVG currentColor does not inherit through <img>; masks retain the UI tone.
export const monochromeProviderIcons = new Set<ProviderIconName>(["anthropic", "cerebras", "codex", "commandcode", "copilot", "cursor", "factory", "grok", "groq", "lmstudio", "mimo", "moonshot", "ollama", "openai", "qoder", "replicate", "xai", "zai", "zed"]);
