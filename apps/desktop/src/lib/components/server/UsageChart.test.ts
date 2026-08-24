// @vitest-environment happy-dom
import { flushSync, mount, unmount } from "svelte";
import { afterEach, expect, test } from "vitest";

import type { UsageTimeseriesPoint } from "../../types";
import { formatCompact } from "../../utils/format";
import UsageChart from "./UsageChart.svelte";

function point(date: string, tokens: number, requestCount = 1): UsageTimeseriesPoint {
  return {
    date,
    requestCount,
    inputTokens: tokens,
    outputTokens: 0,
    cacheReadTokens: 0,
    cacheCreationTokens: 0,
    estimatedCostMicros: 0
  };
}

function todayLocal(): string {
  const today = new Date();
  return [today.getFullYear(), String(today.getMonth() + 1).padStart(2, "0"), String(today.getDate()).padStart(2, "0")].join("-");
}

let app: Record<string, unknown> | undefined;

afterEach(async () => {
  if (app) await unmount(app as never);
  app = undefined;
  document.body.innerHTML = "";
});

function mountChart(series: UsageTimeseriesPoint[]) {
  const target = document.createElement("div");
  document.body.appendChild(target);
  app = mount(UsageChart, { target, props: { series } }) as never;
  flushSync();
}

test("renders y-axis ticks and per-day x labels in the 7-day view", () => {
  mountChart([point(todayLocal(), 2000, 3)]);

  const yLabels = Array.from(
    document.body.querySelectorAll<SVGTextElement>("text.axis-label[text-anchor='end']")
  ).map((node) => node.textContent?.trim());
  expect(yLabels).toEqual([formatCompact(2000), formatCompact(1000)]);

  const xLabels = document.body.querySelectorAll(
    "text.axis-label[text-anchor='middle']"
  );
  // 7-day view labels every day.
  expect(xLabels.length).toBe(7);

  const gridlines = document.body.querySelectorAll("line.gridline");
  expect(gridlines.length).toBe(2);
});

test("30-day view thins x labels and keeps the last day labeled", () => {
  mountChart([point(todayLocal(), 100)]);

  const toggle = Array.from(document.body.querySelectorAll("button")).find(
    (button) => button.textContent?.match(/30|30天|近30/)
  );
  toggle?.click();
  flushSync();

  const xLabels = Array.from(
    document.body.querySelectorAll<SVGTextElement>("text.axis-label[text-anchor='middle']")
  );
  // Every 5 days plus the final day: 0,5,10,15,20,25,29.
  expect(xLabels.length).toBe(7);
  expect(xLabels.at(-1)?.textContent?.trim()).toBe(todayLocal().slice(5));
});

test("day with requests but zero tokens still renders a visible sliver", () => {
  mountChart([point(todayLocal(), 0, 4)]);

  const bars = Array.from(document.body.querySelectorAll<SVGRectElement>("rect.bar"));
  const heights = bars.map((bar) => Number(bar.getAttribute("height")));
  expect(Math.max(...heights)).toBe(2);
});

test("bars stay inside the chart area right of the y axis", () => {
  mountChart([point(todayLocal(), 5000, 2)]);

  const bars = Array.from(document.body.querySelectorAll<SVGRectElement>("rect.bar"));
  for (const bar of bars) {
    expect(Number(bar.getAttribute("x"))).toBeGreaterThanOrEqual(40);
    const height = Number(bar.getAttribute("height"));
    expect(Number(bar.getAttribute("y")) + height).toBeLessThanOrEqual(120);
  }
});

test("stacks model segments and shows a detailed hover tooltip", () => {
  const date = todayLocal();
  mountChart([{
    ...point(date, 300, 3),
    inputTokens: 100,
    outputTokens: 50,
    models: [
      {
        model: "gpt-4o",
        requestCount: 2,
        inputTokens: 80,
        outputTokens: 20,
        cacheReadTokens: 0,
        cacheCreationTokens: 0,
        estimatedCostMicros: 1200
      },
      {
        model: "claude-3-7-sonnet",
        requestCount: 1,
        inputTokens: 20,
        outputTokens: 30,
        cacheReadTokens: 0,
        cacheCreationTokens: 0,
        estimatedCostMicros: 800
      }
    ],
    estimatedCostMicros: 2000
  }]);

  const segments = document.querySelectorAll("rect.bar-segment");
  expect(segments.length).toBe(2);
  expect(segments[0].getAttribute("style")).not.toBe(segments[1].getAttribute("style"));

  const group = Array.from(document.querySelectorAll<SVGGElement>("g.bar-group")).find(
    (node) => node.getAttribute("aria-label")?.startsWith(date)
  );
  expect(group).toBeDefined();
  group?.dispatchEvent(new MouseEvent("mouseenter"));
  flushSync();

  const tooltip = document.querySelector(".chart-tooltip");
  expect(tooltip?.textContent).toContain("gpt-4o");
  expect(tooltip?.textContent).toContain("claude-3-7-sonnet");
  expect(tooltip?.textContent).toContain("150");
});
