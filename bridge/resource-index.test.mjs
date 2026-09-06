import assert from "node:assert/strict";
import { resolve, join, parse } from "node:path";
import { test } from "node:test";
import { ResourceIndex, ResourceSources, pathContains } from "./resource-index.mjs";

const item = (kind, name, path, diagnostics = []) => ({ kind, name, path, id: `${kind}:${path}#${name}`, diagnostics });

test("resource ancestry respects boundaries and filesystem roots", () => {
  assert.equal(pathContains(resolve("a"), resolve("another")), false);
  assert.equal(pathContains(resolve("a"), resolve("a/b")), true);
  assert.equal(pathContains(parse(process.cwd()).root, process.cwd()), true);
  assert.equal(pathContains(undefined, resolve("a")), false);
});

test("several tools and providers from one extension remain independently visible", () => {
  const index = new ResourceIndex();
  for (const kind of ["tool", "provider"]) {
    index.upsert(item(kind, "first", "/extensions/shared.mjs"));
    index.upsert(item(kind, "second", "/extensions/shared.mjs"));
  }
  assert.equal(index.items.length, 4);
  index.upsert(item("tool", "first", "/extensions/shared.mjs", ["warning"]));
  assert.equal(index.items.length, 4);
  assert.deepEqual(index.items[0].diagnostics, ["warning"]);
});

test("upsert merges a declared directory with its first loaded file, not sibling exports", () => {
  const index = new ResourceIndex();
  index.upsert(item("extension", "root", resolve("pkg"), ["a"]));
  index.upsert(item("extension", "one", resolve("pkg/one.mjs"), ["b", "a"]));
  index.upsert(item("extension", "two", resolve("pkg/two.mjs")));
  assert.equal(index.items.length, 2);
  assert.equal(index.items[0].name, "one");
  assert.deepEqual(index.items[0].diagnostics, ["a", "b"]);
});

test("path replacement removes old trie membership and keeps kind isolation", () => {
  const index = new ResourceIndex();
  index.upsert({ ...item("skill", "move", resolve("before")), id: "fixed" });
  index.upsert({ ...item("skill", "move", resolve("after")), id: "fixed" });
  index.upsert(item("skill", "new", resolve("before")));
  index.upsert(item("prompt", "new", resolve("after")));
  assert.equal(index.items.length, 3);
});

test("source provenance prefers exact, then package, then original order", () => {
  const make = (path, origin, source, baseDir) => ({ path: resolve(path), metadata: { origin, source, baseDir } });
  const sources = new ResourceSources({ extensions: [
    make("pkg", "top-level", "loose"), make("pkg", "package", "installed"),
    make("pkg/exact.mjs", "top-level", "exact"),
  ] });
  assert.equal(sources.find("extension", resolve("pkg/other.mjs")).source, "installed");
  assert.equal(sources.find("extension", resolve("pkg/exact.mjs")).source, "exact");
  assert.equal(sources.find("extension", resolve("pkg")).source, "installed");
  assert.equal(sources.find("skill", resolve("pkg/exact.mjs")), undefined);
  assert.equal(sources.find("extension", resolve("pkg-other/file.mjs")), undefined);
});

test("indexed upsert matches the original linear semantics across deterministic path changes", () => {
  const indexed = new ResourceIndex();
  const linear = [];
  let seed = 19;
  for (let step = 0; step < 1200; step++) {
    seed = (Math.imul(seed, 1664525) + 1013904223) >>> 0;
    const path = join(process.cwd(), `pkg${seed % 19}`, ...(seed % 5 ? [`f${seed % 47}`] : []));
    const next = item(["extension", "skill", "prompt"][seed % 3], `entry${seed % 43}`, path, [`d${seed % 7}`]);
    const found = linear.findIndex((candidate) => candidate.kind === next.kind &&
      (pathContains(candidate.path, next.path) || pathContains(next.path, candidate.path) || candidate.id === next.id));
    if (found < 0) linear.push(next);
    else linear[found] = { ...linear[found], ...next,
      diagnostics: [...new Set([...linear[found].diagnostics, ...next.diagnostics])] };
    indexed.upsert(next);
    assert.deepEqual(indexed.items, linear);
  }
});


test("partial updates retain the merged path index", () => {
  const index = new ResourceIndex();
  index.upsert({ ...item("skill", "root", resolve("shared")), id: "stable" });
  index.upsert({ kind: "skill", id: "stable", description: "updated" });
  index.upsert(item("skill", "child", resolve("shared/child")));
  assert.equal(index.items.length, 1);
  assert.equal(index.items[0].description, "updated");
});

test("colliding identities still choose the earliest surviving row after a move", () => {
  const indexed = new ResourceIndex();
  const linear = [];
  const inputs = [
    { kind: "extension", id: "a", path: resolve("package/a") },
    { kind: "extension", id: "b", path: resolve("package/b") },
    { kind: "extension", id: "parent", path: resolve("package") },
    { kind: "extension", id: "b", path: resolve("package/b") },
    { kind: "extension", id: "new", path: resolve("package/b/new") },
    { kind: "extension", id: "b", path: resolve("elsewhere") },
  ];
  for (const next of inputs) {
    const found = linear.findIndex((candidate) => candidate.kind === next.kind &&
      (pathContains(candidate.path, next.path) || pathContains(next.path, candidate.path) || candidate.id === next.id));
    if (found < 0) linear.push(next);
    else linear[found] = { ...linear[found], ...next,
      diagnostics: [...new Set([...(linear[found].diagnostics ?? []), ...(next.diagnostics ?? [])])] };
    indexed.upsert(next);
    assert.deepEqual(indexed.items, linear);
  }
});
