#!/usr/bin/env node
// Reproducible isolated algorithm benchmark, NOT application startup/UI timing.
// Both implementations receive identical rows/queries and must deep-equal first.
import assert from 'node:assert/strict';
import { performance } from 'node:perf_hooks';
import { cpus, platform, arch, totalmem } from 'node:os';
import { resolve, sep } from 'node:path';
import { ResourceIndex, ResourceSources } from '../bridge/resource-index.mjs';

function normalizedPath(value) {
  if (typeof value !== 'string' || !value) return undefined;
  const path = resolve(value);
  return process.platform === 'win32' ? path.toLowerCase() : path;
}
function pathContains(parent, child) {
  const left = normalizedPath(parent), right = normalizedPath(child);
  return !!left && !!right && (left === right || right.startsWith(`${left}${sep}`));
}
// Baseline source functions, extracted from the supplied snapshot. The fixture
// excludes filesystem-root ancestry, whose baseline behavior was incorrect.
function upsertResource(items, item) {
  const index = items.findIndex(candidate => candidate.kind === item.kind &&
    ((candidate.path && item.path && pathContains(candidate.path, item.path)) ||
     (candidate.path && item.path && pathContains(item.path, candidate.path)) || candidate.id === item.id));
  if (index < 0) items.push(item);
  else items[index] = { ...items[index], ...item,
    diagnostics: [...new Set([...(items[index].diagnostics ?? []), ...(item.diagnostics ?? [])])] };
}
function sourceInfo(resource) {
  return { path: resource.path, source: resource.metadata?.source ?? 'unknown',
    scope: resource.metadata?.scope ?? 'user', origin: resource.metadata?.origin ?? 'top-level',
    baseDir: resource.metadata?.baseDir };
}
function findResolvedSource(resources, path) {
  return resources.map(sourceInfo).filter(source => pathContains(source.path, path) ||
    (source.baseDir && pathContains(source.baseDir, path))).sort((a, b) => {
      const ae = normalizedPath(a.path) === normalizedPath(path) ? 0 : 1;
      const be = normalizedPath(b.path) === normalizedPath(path) ? 0 : 1;
      return ae - be || (a.origin === 'package' ? 0 : 1) - (b.origin === 'package' ? 0 : 1);
    })[0];
}
const base = resolve('synthetic-resource-benchmark');
function fixture(count) {
  const rows = Array.from({ length: count }, (_, n) => ({
    id: `extension:${n}`, kind: 'extension', name: `Extension ${n}`,
    path: resolve(base, `package-${n}`, 'extension.mjs'), diagnostics: [],
  }));
  const resources = rows.map((row, n) => ({ path: row.path,
    metadata: { source: `fixture-${n}`, scope: 'user', origin: 'package', baseDir: resolve(row.path, '..') } }));
  const queries = rows.map(row => row.path);
  return { rows, resources, queries };
}
function legacy({ rows, resources, queries }) {
  const items = [];
  for (const row of rows) upsertResource(items, row);
  return { items, sources: queries.map(path => findResolvedSource(resources, path)) };
}
function indexed({ rows, resources, queries }) {
  const index = new ResourceIndex();
  for (const row of rows) index.upsert(row);
  const sources = new ResourceSources({ extensions: resources });
  return { items: index.items, sources: queries.map(path => sources.find('extension', path)) };
}
function measure(run, data) {
  const samples = [];
  for (let i = 0; i < 5; i++) {
    const start = performance.now();
    const result = run(data);
    assert.equal(result.items.length, data.rows.length);
    samples.push(performance.now() - start);
  }
  const sorted = [...samples].sort((a, b) => a - b);
  return { median_ms: sorted[2], slowest_ms: sorted[4], samples_ms: samples };
}
const counts = process.argv.slice(2).map(Number);
if (counts.some(n => !Number.isInteger(n) || n < 1 || n > 10000)) throw new Error('Counts must be integers from 1 to 10000.');
const results = [];
for (const count of counts.length ? counts : [250, 500, 1000]) {
  const data = fixture(count);
  assert.deepEqual(indexed(data), legacy(data));
  // One untimed, identical correctness/warm-up pass above for each algorithm.
  const before = measure(legacy, data), after = measure(indexed, data);
  results.push({ resources: count, sources: count, queries: count, outputs_equal: true,
    baseline: before, indexed: after, median_speedup: before.median_ms / after.median_ms });
}
console.log(JSON.stringify({
  workload: 'Build deduplicated extension rows and resolve their provenance; no disk, Pi, GPUI, or network.',
  runtime: process.version, v8: process.versions.v8, os: platform(), arch: arch(),
  cpu: cpus()[0]?.model, logical_cpus: cpus().length, system_memory_bytes: totalmem(),
  iterations: 5, results,
}, null, 2));
