import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { readFile, readdir } from "node:fs/promises";
import { resolve } from "node:path";
import test from "node:test";

const root = resolve(import.meta.dirname, "../../vendor");
const manifest = JSON.parse(await readFile(resolve(root, "upstream.json")));
const sha = bytes => createHash("sha256").update(bytes).digest("hex");
async function files(directory, prefix = "") {
  const result = [];
  for (const entry of await readdir(directory, { withFileTypes: true })) {
    assert(!entry.isSymbolicLink(), "Vendored files must have explicit contents");
    const path = prefix + entry.name;
    if (entry.isDirectory()) result.push(...await files(resolve(directory, entry.name), path + "/"));
    else result.push(path);
  }
  return result.sort();
}
for (const pkg of manifest.packages) test(`${pkg.directory}: complete pinned source inventory`, async () => {
  const directory = resolve(root, pkg.directory);
  assert(pkg.repository.startsWith("https://"));
  assert(pkg.revision || pkg.archiveSha256);
  assert.deepEqual(await files(directory), Object.keys(pkg.files).sort());
  for (const [path, record] of Object.entries(pkg.files)) {
    assert.equal(sha(await readFile(resolve(directory, path))), record.sha256, path);
    if (record.upstreamSha256) assert.match(record.upstreamSha256, /^[a-f0-9]{64}$/);
  }
  assert.deepEqual(pkg.changedFiles.sort(), Object.entries(pkg.files)
    .filter(([, r]) => r.sha256 !== r.upstreamSha256).map(([path]) => path).sort());
  assert(Object.keys(pkg.files).some(path => path.startsWith("LICENSE")));
  assert((await readFile(resolve(root, "patches", pkg.directory + ".patch"))).length > 0);
});
