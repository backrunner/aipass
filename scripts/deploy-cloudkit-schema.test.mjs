import { test } from "node:test";
import assert from "node:assert/strict";
import { mkdtemp, readFile, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { deploySchema, mergeSchema } from "./deploy-cloudkit-schema.mjs";

const required = await readFile(new URL("../infra/cloudkit/schema.ckdb", import.meta.url), "utf8");
const baseline = 'DEFINE SCHEMA\n RECORD TYPE Users (roles LIST<INT64>, GRANT READ TO "_world");\n RECORD TYPE Unrelated (value STRING);\n';

test("schema merge preserves server types, fields, and indexes and is idempotent", () => {
  const merged = mergeSchema(baseline, required);
  assert.ok(merged.startsWith(baseline));
  assert.equal(mergeSchema(merged, required), merged);
  const partial = `${baseline} RECORD TYPE VaultSnapshot (existing STRING QUERYABLE, GRANT READ TO "_creator");`;
  const completed = mergeSchema(partial, required);
  assert.ok(completed.includes("existing STRING QUERYABLE"));
  assert.ok(completed.includes("ciphertext ASSET"));
  assert.ok(completed.includes('GRANT WRITE TO "_creator"'));
  assert.equal(mergeSchema(completed, required), completed);
  assert.throws(() => mergeSchema(required.replace("ASSET", "STRING"), required), /incompatible/);
  assert.throws(() => mergeSchema(required.replaceAll('"_creator"', '"_world"'), required), /permissions/);
});

async function fixture(t, fail) {
  const output = await mkdtemp(join(tmpdir(), "aipass-schema-test-"));
  t.after(() => rm(output, { recursive: true, force: true }));
  const calls = [];
  const schemas = { development: baseline, production: baseline };
  const run = async (args) => {
    const operation = args[0];
    const environment = args[args.indexOf("--environment") + 1];
    calls.push(`${operation}:${environment}`);
    if (fail === `${operation}:${environment}`) throw new Error("authorization-failed");
    if (operation === "export-schema") await writeFile(args[args.indexOf("--output-file") + 1], schemas[environment]);
    if (operation === "import-schema") schemas[environment] = await readFile(args[args.indexOf("--file") + 1], "utf8");
  };
  return { options: { team: "TESTTEAM00", output, deploy: true, run }, calls, schemas };
}

test("deployment exports and validates both environments before importing and verifies afterward", async (t) => {
  const { options, calls, schemas } = await fixture(t);
  const report = await deploySchema(options);
  assert.equal(report.deployed, true);
  assert.deepEqual(calls, ["export-schema:development", "export-schema:production", "validate-schema:development", "validate-schema:production", "import-schema:development", "export-schema:development", "import-schema:production", "export-schema:production"]);
  assert.equal(schemas.production, mergeSchema(baseline, required));
  calls.length = 0;
  await deploySchema(options);
  assert.ok(calls.every((call) => !call.startsWith("import-schema:")));
});

test("auth and production validation failures perform no imports", async (t) => {
  for (const failure of ["export-schema:development", "export-schema:production", "validate-schema:production"]) {
    const { options, calls } = await fixture(t, failure);
    await assert.rejects(deploySchema(options), /authorization-failed/);
    assert.ok(calls.every((call) => !call.startsWith("import-schema:")));
  }
});

test("default invocation validates without mutation and a stale post-import schema fails verification", async (t) => {
  const { options, calls } = await fixture(t);
  const report = await deploySchema({ ...options, deploy: false });
  assert.equal(report.deployed, false);
  assert.ok(calls.every((call) => !call.startsWith("import-schema:")));
  const run = async (args) => { if (args[0] !== "import-schema") await options.run(args); };
  await assert.rejects(deploySchema({ ...options, run }), /verification failed/);
});
