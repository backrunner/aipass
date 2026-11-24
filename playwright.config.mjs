import { defineConfig } from "@playwright/test";

const surfaces = [
  { name: "desktop", directory: "apps/desktop/dist", port: 4181 },
  { name: "panel", directory: "apps/control-panel/embedded", port: 4182 },
  { name: "extension", directory: "apps/extension/dist", port: 4183, entry: "/popup.html" },
  { name: "web", directory: "apps/web/build", port: 4184 },
];

const portOffset = Number(process.env.AIPASS_E2E_PORT_OFFSET || 0);
if (!Number.isSafeInteger(portOffset) || portOffset < 0 || portOffset > 65535 - 4184) {
  throw new Error("AIPASS_E2E_PORT_OFFSET must be an integer between 0 and 61351");
}
const portFor = ({ port }) => port + portOffset;
const outputRoot = portOffset ? `artifacts/ui-e2e-${portOffset}/` : "";
const origin = (surface) => `http://127.0.0.1:${portFor(surface)}`;
const projects = surfaces.map((surface) => ({
  name: surface.name,
  testMatch: `${surface.name}*.spec.mjs`,
  use: { baseURL: origin(surface) },
}));

export default defineConfig({
  testDir: "./scripts/ui-e2e",
  fullyParallel: true,
  workers: 4,
  forbidOnly: !!process.env.CI,
  outputDir: outputRoot + "test-results",
  reporter: [["list"], ["html", { open: "never", outputFolder: outputRoot + "playwright-report" }]],
  use: {
    viewport: { width: 960, height: 640 },
    locale: "en-US",
    screenshot: "only-on-failure",
    trace: "retain-on-failure",
  },
  projects: [
    ...projects,
    ...projects.filter(({ name }) => name !== "extension").map(project => ({
      ...project, name: project.name + "-webkit", use: { ...project.use, browserName: "webkit" },
    })),
  ],
  webServer: surfaces.filter(({ name }) => name !== "panel").map((surface) => ({
    name: surface.name,
    command: `pnpm exec vite preview --outDir ${surface.directory} --host 127.0.0.1 --port ${portFor(surface)} --strictPort`,
    url: origin(surface) + (surface.entry ?? "/"),
  })),
});
