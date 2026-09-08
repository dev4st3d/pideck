#!/usr/bin/env node
/** Import the exact design fonts. No system-font substitution is accepted. */
import { readFile, writeFile, mkdir } from 'node:fs/promises';
import { createHash } from 'node:crypto';
import { inflateRawSync } from 'node:zlib';
import { resolve, dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const LIMIT = 8 * 1024 * 1024;

export function verifyFont(bytes, font) {
  const sha = createHash('sha256').update(bytes).digest('hex');
  if (sha !== font.sha256) throw new Error(`SHA-256 mismatch for ${font.file}; use the original design ZIP`);
  return sha;
}

export function extractFont(archive, name) {
  // Read the central directory; never write paths supplied by an archive.
  function bounds(offset, length) {
    if (offset < 0 || length < 0 || offset + length > archive.length) {
      throw new Error('Invalid ZIP: entry outside archive');
    }
  }
  let eocd = -1;
  for (let i = archive.length - 22; i >= Math.max(0, archive.length - 65557); i--) {
    if (archive.readUInt32LE(i) === 0x06054b50 && i + 22 + archive.readUInt16LE(i + 20) === archive.length) {
      eocd = i; break;
    }
  }
  if (eocd < 0) throw new Error('Invalid ZIP: missing central directory');
  if (archive.readUInt16LE(eocd + 4) || archive.readUInt16LE(eocd + 6)) {
    throw new Error('Split ZIP archives are not supported');
  }
  const count = archive.readUInt16LE(eocd + 10);
  let offset = archive.readUInt32LE(eocd + 16);
  bounds(offset, archive.readUInt32LE(eocd + 12));
  for (let i = 0; i < count; i++) {
    bounds(offset, 46);
    if (archive.readUInt32LE(offset) !== 0x02014b50) throw new Error('Invalid ZIP entry');
    const flags = archive.readUInt16LE(offset + 8);
    const method = archive.readUInt16LE(offset + 10);
    const size = archive.readUInt32LE(offset + 20);
    const unpacked = archive.readUInt32LE(offset + 24);
    const nameLength = archive.readUInt16LE(offset + 28);
    const extraLength = archive.readUInt16LE(offset + 30);
    const commentLength = archive.readUInt16LE(offset + 32);
    const local = archive.readUInt32LE(offset + 42);
    bounds(offset + 46, nameLength + extraLength + commentLength);
    const entry = archive.subarray(offset + 46, offset + 46 + nameLength).toString('utf8');
    offset += 46 + nameLength + extraLength + commentLength;
    if (!(entry === name || entry.endsWith(`/fonts/${name}`))) continue;
    if ((flags & 1) || unpacked > LIMIT || size > LIMIT) throw new Error(`Unsafe font entry ${entry}`);
    bounds(local, 30);
    if (archive.readUInt32LE(local) !== 0x04034b50) throw new Error('Invalid local ZIP header');
    const start = local + 30 + archive.readUInt16LE(local + 26) + archive.readUInt16LE(local + 28);
    bounds(start, size);
    const bytes = archive.subarray(start, start + size);
    const result = method === 0 ? bytes : method === 8
      ? inflateRawSync(bytes, { maxOutputLength: LIMIT }) : null;
    if (!result || result.length !== unpacked) throw new Error(`Unsupported/corrupt font ${entry}`);
    return result;
  }
  throw new Error(`The design ZIP does not contain ${name}`);
}

export async function main(args = process.argv.slice(2)) {
  if (args.length && (args.length !== 2 || !['--zip', '--dir'].includes(args[0]))) {
    throw new Error('Usage: node scripts/prepare-fonts.mjs [--zip pideck-design.zip | --dir design/fonts]');
  }
  const destination = join(root, 'assets', 'fonts');
  const manifest = JSON.parse(await readFile(join(destination, 'sources.json'), 'utf8'));
  const archive = args[0] === '--zip' ? await readFile(resolve(args[1])) : null;
  // Validate the entire set before changing any local font file.
  const verified = [];
  for (const font of manifest) {
    let bytes;
    if (archive) bytes = extractFont(archive, font.file);
    else if (args[0] === '--dir') bytes = await readFile(join(resolve(args[1]), font.file));
    else {
      try { bytes = await readFile(join(destination, font.file)); }
      catch (error) {
        if (error.code !== 'ENOENT') throw error;
        const response = await fetch(font.source, { signal: AbortSignal.timeout(30000) });
        if (!response.ok) throw new Error(`Font download returned HTTP ${response.status}`);
        const declaredSize = Number(response.headers.get('content-length') || 0);
        if (declaredSize > LIMIT) throw new Error(`Oversized font download: ${font.file}`);
        const chunks = []; let size = 0;
        if (!response.body) throw new Error(`Empty font download: ${font.file}`);
        for await (const chunk of response.body) {
          size += chunk.length;
          if (size > LIMIT) throw new Error(`Oversized font download: ${font.file}`);
          chunks.push(chunk);
        }
        bytes = Buffer.concat(chunks);
      }
    }
    const sha = verifyFont(bytes, font);
    verified.push({ font, bytes, sha });
  }
  await mkdir(destination, { recursive: true });
  for (const { font, bytes, sha } of verified) {
    await writeFile(join(destination, font.file), bytes);
    console.log(`Verified ${font.file}: ${sha}`);
  }
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  main().catch(error => { console.error(error.message); process.exitCode = 1; });
}
