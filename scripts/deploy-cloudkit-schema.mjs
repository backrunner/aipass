import { execFileSync } from "node:child_process";
import { mkdir, mkdtemp, readFile, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import { parseArgs } from "node:util";
import { defaultContainer } from "./cloudkit-profile.mjs";

const schemaPath = fileURLToPath(new URL("../infra/cloudkit/schema.ckdb", import.meta.url));
const recordPattern = /\bRECORD\s+TYPE\s+"?VaultSnapshot"?\s*\(([^]*?)\);/g;

// Importing a minimal schema can remove development types. Always add the
// required record to a fresh export, retaining all existing declarations.
export function mergeSchema(current, required) {
  if (!/^\s*DEFINE\s+SCHEMA\b/.test(current)) throw new Error("Unrecognized CloudKit schema export");
  const definition = [...required.matchAll(recordPattern)];
  if (definition.length !== 1) throw new Error("Expected one required VaultSnapshot definition");
  const existing = [...current.matchAll(recordPattern)];
  if (existing.length > 1) throw new Error("Duplicate VaultSnapshot definitions");
  if (existing.length === 0) return `${current.trimEnd()}\n\n    ${definition[0][0]}\n`;
  const [record, body] = existing[0];
  const field = body.match(/(?:^|,)\s*"?ciphertext"?\s+([A-Z0-9_<>]+)/);
  if (field && field[1] !== "ASSET") throw new Error("VaultSnapshot.ciphertext already has an incompatible type");
  const grants = [...body.matchAll(/GRANT\s+(\w+)\s+TO\s+"([^"]+)"/g)];
  if (grants.some((grant) => grant[2] !== "_creator")) {
    throw new Error("VaultSnapshot has non-creator permissions; review the exported schema before deployment");
  }
  const additions = [];
  if (!field) additions.push("ciphertext ASSET");
  for (const permission of ["WRITE", "READ"]) {
    if (!grants.some((grant) => grant[1] === permission && grant[2] === "_creator")) {
      additions.push(`GRANT ${permission} TO "_creator"`);
    }
  }
  if (!additions.length) return current;
  // Fields must precede grants in ckdb syntax.
  let merged = body;
  if (!field) merged = `\n        ciphertext ASSET,${merged}`;
  const missingGrants = additions.filter((item) => item.startsWith("GRANT"));
  if (missingGrants.length) merged = `${merged.trimEnd().replace(/,$/, "")},\n        ${missingGrants.join(",\n        ")}\n    `;
  return current.replace(record, () => record.replace(body, () => merged));
}

export async function deploySchema({ team, container = defaultContainer, output, deploy = false, run = runCktool }) {
  if (!/^[A-Z0-9]{10}$/.test(team ?? "")) throw new Error("Pass --team with the Apple Developer team ID");
  if (!/^iCloud\.[A-Za-z0-9.-]+$/.test(container)) throw new Error("Invalid CloudKit container ID");
  const directory = output ? resolve(output) : await mkdtemp(join(tmpdir(), "aipass-cloudkit-schema-"));
  await mkdir(directory, { recursive: true, mode: 0o700 });
  const required = await readFile(schemaPath, "utf8");
  const plans = [];
  for (const environment of ["development", "production"]) {
    const scope = ["--team-id", team, "--container-id", container, "--environment", environment];
    const before = join(directory, `${environment}-before.ckdb`);
    const planned = join(directory, `${environment}-planned.ckdb`);
    await run(["export-schema", ...scope, "--output-file", before]);
    const current = await readFile(before, "utf8");
    const merged = mergeSchema(current, required);
    await writeFile(planned, merged, { mode: 0o600 });
    plans.push({ environment, scope, planned, changed: merged !== current });
  }
  // Validate BOTH environments before the first mutation. Auth/validation
  // failures cannot leave a partially imported development schema.
  for (const plan of plans) await run(["validate-schema", ...plan.scope, "--file", plan.planned]);
  if (deploy) {
    for (const plan of plans) {
      if (plan.changed) await run(["import-schema", ...plan.scope, "--validate", "--file", plan.planned]);
      const after = join(directory, `${plan.environment}-after.ckdb`);
      await run(["export-schema", ...plan.scope, "--output-file", after]);
      const actual = await readFile(after, "utf8");
      if (mergeSchema(actual, required) !== actual) throw new Error(`${plan.environment} schema verification failed`);
    }
  }
  const report = { team, container, deployed: deploy, directory, environments: plans.map(({ environment, changed }) => ({ environment, changed })) };
  await writeFile(join(directory, "report.json"), `${JSON.stringify(report, null, 2)}\n`, { mode: 0o600 });
  return report;
}

function runCktool(args) {
  // Use cktool's saved management credential. Never put a token on argv or in
  // the deployment report. DEVELOPER_DIR may select Xcode for this process only.
  console.log(`cktool ${args[0]} (${args[args.indexOf("--environment") + 1]})`);
  execFileSync("/usr/bin/xcrun", ["cktool", ...args], { stdio: "inherit" });
}

if (process.argv[1] && import.meta.url === pathToFileURL(resolve(process.argv[1])).href) {
  try {
    const { values } = parseArgs({ options: {
      team: { type: "string" }, container: { type: "string", default: process.env.AIPASS_CLOUDKIT_CONTAINER || defaultContainer },
      output: { type: "string" }, deploy: { type: "boolean", default: false },
    } });
    console.log(JSON.stringify(await deploySchema(values), null, 2));
  } catch (error) {
    console.error(error.message);
    process.exitCode = 1;
  }
}
