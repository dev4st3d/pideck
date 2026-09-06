// Per-inventory indexes. Nothing here is cached across a resource generation.
// Path ancestry is segment-based: /tools/a must never match /tools/another.
import { resolve, sep } from "node:path";

export function normalizedPath(value) {
  if (typeof value !== "string" || !value) return undefined;
  const path = resolve(value);
  return process.platform === "win32" ? path.toLowerCase() : path;
}

export function pathContains(parent, child) {
  const left = normalizedPath(parent);
  const right = normalizedPath(child);
  if (!left || !right) return false;
  return right === left || right.startsWith(left.endsWith(sep) ? left : `${left}${sep}`);
}

const node = () => ({ children: new Map(), exact: new Set(), first: Infinity });
const parts = (path) => path.split(sep).filter(Boolean);
const minimum = (values) => {
  let result = Infinity;
  for (const value of values) result = Math.min(result, value);
  return result;
};

class PathIndex {
  root = node();

  add(path, index) {
    let current = this.root;
    current.first = Math.min(current.first, index);
    for (const part of parts(path)) {
      if (!current.children.has(part)) current.children.set(part, node());
      current = current.children.get(part);
      current.first = Math.min(current.first, index);
    }
    current.exact.add(index);
  }

  remove(path, index) {
    const ancestors = [this.root];
    let current = this.root;
    for (const part of parts(path)) {
      current = current.children.get(part);
      if (!current) return;
      ancestors.push(current);
    }
    current.exact.delete(index);
    for (const ancestor of ancestors.reverse()) {
      ancestor.first = Math.min(minimum(ancestor.exact), minimum(
        [...ancestor.children.values()].map((child) => child.first),
      ));
      for (const [key, child] of ancestor.children) {
        if (child.first === Infinity) ancestor.children.delete(key);
      }
    }
  }

  // Return the earliest related row, matching the old linear upsert's ordering.
  related(path) {
    let current = this.root;
    let best = minimum(current.exact);
    for (const part of parts(path)) {
      current = current.children.get(part);
      if (!current) return best;
      best = Math.min(best, minimum(current.exact));
    }
    return Math.min(best, current.first);
  }
}

export class ResourceIndex {
  items = [];
  #ids = new Map();
  #paths = new Map();
  #locations = [];

  upsert(item) {
    const key = `${item.kind}\0${item.id}`;
    const path = normalizedPath(item.path);
    // Multiple tools/providers may originate in ONE extension file. Their
    // exported names identify them; path-only deduplication silently lost rows.
    const namedExport = item.kind === "tool" || item.kind === "provider";
    const byPath = !namedExport && path ? this.#paths.get(item.kind)?.related(path) : undefined;
    let index = Math.min(minimum(this.#ids.get(key) ?? []), byPath ?? Infinity);
    if (index === Infinity) {
      index = this.items.length;
      this.items.push(item);
    } else {
      const previous = this.items[index];
      const previousKey = `${previous.kind}\0${previous.id}`;
      const identities = this.#ids.get(previousKey);
      identities?.delete(index);
      if (identities?.size === 0) this.#ids.delete(previousKey);

      this.items[index] = {
        ...previous, ...item,
        diagnostics: [...new Set([...(previous.diagnostics ?? []), ...(item.diagnostics ?? [])])],
      };
    }
    if (!this.#ids.has(key)) this.#ids.set(key, new Set());
    this.#ids.get(key).add(index);
    const mergedPath = namedExport ? undefined : normalizedPath(this.items[index].path);
    const previousPath = this.#locations[index];
    if (previousPath && previousPath !== mergedPath) this.#paths.get(item.kind)?.remove(previousPath, index);
    if (mergedPath && previousPath !== mergedPath) {
      if (!this.#paths.has(item.kind)) this.#paths.set(item.kind, new PathIndex());
      this.#paths.get(item.kind).add(mergedPath, index);
    }
    this.#locations[index] = mergedPath;
    return this.items[index];
  }
}

const sourceNode = () => ({ children: new Map(), package: Infinity, other: Infinity });

/** Resolve provenance once per inventory, not map/filter/sort for every row. */
export class ResourceSources {
  #kinds = new Map();

  constructor(resolved) {
    for (const [kind, resources] of [
      ["extension", resolved.extensions ?? []], ["skill", resolved.skills ?? []],
      ["prompt", resolved.prompts ?? []], ["theme", resolved.themes ?? []],
    ]) {
      const state = { root: sourceNode(), exact: new Map(), sources: [] };
      for (const resource of resources) {
        const source = {
          path: resource.path, source: resource.metadata?.source ?? "unknown",
          scope: resource.metadata?.scope ?? "user", origin: resource.metadata?.origin ?? "top-level",
          baseDir: resource.metadata?.baseDir,
        };
        const index = state.sources.length;
        state.sources.push(source);
        const priority = source.origin === "package" ? "package" : "other";
        const exact = normalizedPath(source.path);
        if (exact) {
          const previous = state.exact.get(exact);
          if (previous === undefined || (priority === "package" && state.sources[previous].origin !== "package")) {
            state.exact.set(exact, index);
          }
        }
        for (const path of new Set([exact, normalizedPath(source.baseDir)])) {
          if (!path) continue;
          let current = state.root;
          for (const part of parts(path)) {
            if (!current.children.has(part)) current.children.set(part, sourceNode());
            current = current.children.get(part);
          }
          current[priority] = Math.min(current[priority], index);
        }
      }
      this.#kinds.set(kind, state);
    }
  }

  find(kind, path) {
    const state = this.#kinds.get(kind);
    const normalized = normalizedPath(path);
    if (!state || !normalized) return undefined;
    const exact = state.exact.get(normalized);
    if (exact !== undefined) return state.sources[exact];
    let current = state.root;
    let bestPackage = current.package;
    let bestOther = current.other;
    for (const part of parts(normalized)) {
      current = current.children.get(part);
      if (!current) break;
      bestPackage = Math.min(bestPackage, current.package);
      bestOther = Math.min(bestOther, current.other);
    }
    return state.sources[bestPackage === Infinity ? bestOther : bestPackage];
  }
}
