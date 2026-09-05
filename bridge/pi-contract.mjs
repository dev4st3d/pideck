import { readFileSync, realpathSync } from "node:fs";
import { isAbsolute, relative, resolve, sep } from "node:path";
import { pathToFileURL } from "node:url";

export const PI_PACKAGE = "@earendil-works/pi-coding-agent";
export const PI_VERSION = "0.85.1";
export const MIN_NODE_VERSION = "22.19.0";

/** Verify identity and the published public export before importing any code. */
export function resolveSdkEntry(packageRoot) {
  const root = realpathSync(packageRoot);
  const manifest = JSON.parse(readFileSync(resolve(root, "package.json"), "utf8"));
  if (manifest.name !== PI_PACKAGE || manifest.version !== PI_VERSION
      || manifest.bin?.pi !== "dist/bundle/cli.js"
      || manifest.exports?.["."]?.import !== "./dist/index.js") {
    throw new Error(`Requires ${PI_PACKAGE}@${PI_VERSION} with its published SDK export`);
  }
  const entry = realpathSync(resolve(root, manifest.exports["."].import));
  const path = relative(root, entry);
  if (path === ".." || path.startsWith(`..${sep}`) || isAbsolute(path)) {
    throw new Error("The Pi SDK export must stay within its package");
  }
  return { version: manifest.version, entry: pathToFileURL(entry).href };
}
