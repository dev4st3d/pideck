import test from 'node:test';
import assert from 'node:assert/strict';
import { deflateRawSync } from 'node:zlib';
import { createHash } from 'node:crypto';
import { extractFont, verifyFont, main } from './prepare-fonts.mjs';

// Synthetic bytes, not a redistributed font. Hash validation is tested separately.
function archive(name, data, method = 0) {
  const filename = Buffer.from(name);
  const payload = method === 8 ? deflateRawSync(data) : data;
  const local = Buffer.alloc(30);
  local.writeUInt32LE(0x04034b50, 0); local.writeUInt16LE(method, 8);
  local.writeUInt32LE(payload.length, 18); local.writeUInt32LE(data.length, 22);
  local.writeUInt16LE(filename.length, 26);
  const central = Buffer.alloc(46);
  central.writeUInt32LE(0x02014b50, 0); central.writeUInt16LE(method, 10);
  central.writeUInt32LE(payload.length, 20); central.writeUInt32LE(data.length, 24);
  central.writeUInt16LE(filename.length, 28);
  const end = Buffer.alloc(22);
  end.writeUInt32LE(0x06054b50, 0); end.writeUInt16LE(1, 8); end.writeUInt16LE(1, 10);
  end.writeUInt32LE(central.length + filename.length, 12);
  end.writeUInt32LE(local.length + filename.length + payload.length, 16);
  return Buffer.concat([local, filename, payload, central, filename, end]);
}
const data = Buffer.from('synthetic font parser test payload');
for (const method of [0, 8]) {
  test(`extracts exact bytes from ${method === 8 ? 'deflated' : 'stored'} design/fonts entry`, () => {
    assert.deepEqual(extractFont(archive('design/fonts/test.ttf', data, method), 'test.ttf'), data);
  });
}
test('accepts a root font entry and ignores unrelated names', () => {
  assert.deepEqual(extractFont(archive('test.ttf', data), 'test.ttf'), data);
  assert.throws(() => extractFont(archive('other/not-test.ttf', data), 'test.ttf'), /does not contain/);
});
test('rejects missing, truncated and out-of-bounds directories', () => {
  assert.throws(() => extractFont(Buffer.alloc(0), 'test.ttf'), /Invalid ZIP/);
  const broken = archive('test.ttf', data);
  broken.writeUInt32LE(0xffffff00, broken.length - 6);
  assert.throws(() => extractFont(broken, 'test.ttf'), /Invalid ZIP/);
});
test('rejects encrypted, oversized and unsupported-method entries', () => {
  for (const kind of ['encrypted', 'oversized', 'method']) {
    const zip = archive('test.ttf', data);
    const central = zip.readUInt32LE(zip.length - 6);
    if (kind === 'encrypted') zip.writeUInt16LE(1, central + 8);
    if (kind === 'oversized') zip.writeUInt32LE(9 * 1024 * 1024, central + 24);
    if (kind === 'method') zip.writeUInt16LE(99, central + 10);
    assert.throws(() => extractFont(zip, 'test.ttf'), /Unsafe|Unsupported/);
  }
});
test('rejects incorrect expanded lengths', () => {
  const zip = archive('test.ttf', data, 8);
  const central = zip.readUInt32LE(zip.length - 6);
  zip.writeUInt32LE(data.length + 1, central + 24);
  assert.throws(() => extractFont(zip, 'test.ttf'), /corrupt/);
});
test('SHA verification accepts only the exact manifest bytes', () => {
  const font = { file: 'test.ttf', sha256: createHash('sha256').update(data).digest('hex') };
  assert.equal(verifyFont(data, font), font.sha256);
  assert.throws(() => verifyFont(Buffer.concat([data, Buffer.from('!')]), font), /SHA-256 mismatch/);
});
test('unknown command line flags fail without downloading or writing', async () => {
  await assert.rejects(main(['--unknown', '/tmp/no']), /Usage:/);
});
