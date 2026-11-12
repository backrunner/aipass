import { lstat, readlink, symlink } from "node:fs/promises";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const link = fileURLToPath(new URL("./reference/node_modules", import.meta.url));
const target = fileURLToPath(new URL("../../apps/desktop/node_modules", import.meta.url));
await lstat(target); // Install workspace dependencies before preparing the reference.
try {
  const stat = await lstat(link);
  if (!stat.isSymbolicLink() || resolve(dirname(link), await readlink(link)) !== target) {
    throw new Error(`Refusing to replace existing dependency path: ${link}`);
  }
} catch (error) {
  if (error.code !== "ENOENT") throw error;
  await symlink(process.platform === "win32" ? target : "../../../apps/desktop/node_modules", link, process.platform === "win32" ? "junction" : "dir");
}
