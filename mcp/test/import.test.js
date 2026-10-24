// scripts/import-image-assets.mjs — 합성 PNG 로만 돈다(실제 생성 사진·static/images 를 읽지 않는다).

import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { mkdir, readFile, readdir, symlink, writeFile } from 'node:fs/promises';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { test } from 'node:test';
import { crc32 } from 'node:zlib';

import { loadImageAssets } from '../src/image-assets.js';
import { PHOTO_ASSET_KEYS } from '../src/photo-sets.js';
import { pngChunks } from '../scripts/import-image-assets.mjs';
import { makePng, tempDir } from './helpers.js';

const SCRIPT = fileURLToPath(new URL('../scripts/import-image-assets.mjs', import.meta.url));
const UUID = (n) => `0000000${n}-aaaa-4bbb-8ccc-dddddddddddd.png`;

/** IEND 앞에 tEXt·caBX 청크를 끼운 PNG(생성 원본의 C2PA 출처 정보 흉내). */
function pngWithMetadata(rgb) {
  const png = makePng(20, 30, rgb);
  const chunk = (type, data) => {
    const body = Buffer.concat([Buffer.from(type, 'ascii'), data]);
    const len = Buffer.alloc(4);
    len.writeUInt32BE(data.length);
    const crc = Buffer.alloc(4);
    crc.writeUInt32BE(crc32(body));
    return Buffer.concat([len, body, crc]);
  };
  const iend = png.length - 12;
  return Buffer.concat([
    png.subarray(0, iend),
    chunk('tEXt', Buffer.from('Software\0test')),
    chunk('caBX', Buffer.from('c2pa manifest (test)')),
    png.subarray(iend),
  ]);
}

function run(args) {
  return spawnSync(process.execPath, [SCRIPT, ...args], { encoding: 'utf8' });
}

test('pngChunks: 청크 이름을 순서대로 읽는다(caBX 확인용)', () => {
  assert.deepEqual(pngChunks(pngWithMetadata([10, 20, 30])), ['IHDR', 'IDAT', 'tEXt', 'caBX', 'IEND']);
  assert.equal(pngChunks(Buffer.from('not an image')), null);
});

test('가져오기: 허용된 키·UUID.png 만, 원본 PNG 를 바이트 그대로(C2PA caBX 포함) 해시 이름으로 복사하고 manifest 를 만든다', async () => {
  const t = await tempDir();
  try {
    const staticDir = join(t.dir, 'static');
    const out = join(t.dir, 'out');
    await mkdir(staticDir);
    const assets = {};
    for (const [i, key] of PHOTO_ASSET_KEYS.entries()) {
      await writeFile(join(staticDir, UUID(i)), pngWithMetadata([i * 40, 50, 90]));
      assets[key] = { static_file: UUID(i), source: { note: `test ${i}` } };
    }
    await writeFile(join(t.dir, 'sources.json'), JSON.stringify({ assets }));
    const r = run(['--sources', join(t.dir, 'sources.json'), '--static-dir', staticDir, '--out', out]);
    assert.equal(r.status, 0, r.stderr);

    const loaded = await loadImageAssets(out);
    assert.deepEqual(loaded.problems, []);
    assert.equal(loaded.byKey.size, PHOTO_ASSET_KEYS.length);
    const manifest = JSON.parse(await readFile(join(out, 'manifest.json'), 'utf8'));
    for (const [i, key] of PHOTO_ASSET_KEYS.entries()) {
      const entry = manifest.assets[key];
      const copied = await readFile(join(out, 'images', entry.file));
      const original = await readFile(join(staticDir, UUID(i)));
      assert.deepEqual(copied, original, `${key}: 바이트 그대로`);
      assert.ok(pngChunks(copied).includes('caBX'), `${key}: C2PA 청크 보존`);
      assert.match(entry.file, /^[0-9a-f]{32}\.png$/, '원본 UUID 가 아닌 해시 이름');
      assert.equal(entry.mime, 'image/png');
      assert.equal(entry.sha256, entry.source.original_sha256);
      assert.deepEqual(entry.provenance, { byte_identical_to_original: true, c2pa_chunk_present: true });
      assert.equal(entry.source.static_file, UUID(i));
    }
  } finally {
    await t.cleanup();
  }
});

test('다시 가져오면 manifest 에 없는 옛 파일(예전 축소 JPEG)은 지워지고 내보내지지 않는다', async () => {
  const t = await tempDir();
  try {
    const staticDir = join(t.dir, 'static');
    const out = join(t.dir, 'out');
    await mkdir(staticDir);
    await mkdir(join(out, 'images'), { recursive: true });
    const stale = `${'a'.repeat(32)}.jpg`;
    await writeFile(join(out, 'images', stale), Buffer.from([0xff, 0xd8, 0xff, 0xe0]));
    const key = PHOTO_ASSET_KEYS[0];
    await writeFile(join(staticDir, UUID(0)), pngWithMetadata([1, 2, 3]));
    await writeFile(join(t.dir, 'sources.json'), JSON.stringify({ assets: { [key]: { static_file: UUID(0) } } }));
    const r = run(['--sources', join(t.dir, 'sources.json'), '--static-dir', staticDir, '--out', out]);
    assert.equal(r.status, 0, r.stderr);
    const files = await readdir(join(out, 'images'));
    assert.equal(files.length, 1);
    assert.ok(!files.includes(stale));
    const loaded = await loadImageAssets(out);
    assert.deepEqual([...loaded.byName.keys()], files);
  } finally {
    await t.cleanup();
  }
});

test('가져오기 거절: 경로가 든 이름·심볼릭 링크·모르는 키·PNG 아님', async () => {
  const t = await tempDir();
  try {
    const staticDir = join(t.dir, 'static');
    await mkdir(staticDir);
    await writeFile(join(t.dir, '.env'), 'SECRET=1');
    await symlink(join(t.dir, '.env'), join(staticDir, UUID(1)));
    await writeFile(join(staticDir, UUID(2)), 'not a png');
    await writeFile(join(staticDir, UUID(3)), makePng(2, 2, [1, 1, 1]));
    const key = PHOTO_ASSET_KEYS[0];
    const cases = [
      [{ [key]: { static_file: '../.env' } }, /UUID\.png/],
      [{ [key]: { static_file: 'user-11111111-2222-4333-8444-555555555555.png' } }, /UUID\.png/],
      [{ [key]: { static_file: UUID(1) } }, /일반 파일이 아님/],
      [{ [key]: { static_file: UUID(2) } }, /PNG 가 아님/],
      [{ 'not-a-key': { static_file: UUID(3) } }, /asset key/],
    ];
    for (const [assets, why] of cases) {
      await writeFile(join(t.dir, 'sources.json'), JSON.stringify({ assets }));
      const r = run(['--sources', join(t.dir, 'sources.json'), '--static-dir', staticDir, '--out', join(t.dir, 'out')]);
      assert.notEqual(r.status, 0, JSON.stringify(assets));
      assert.match(r.stderr, why);
      assert.doesNotMatch(r.stderr, /SECRET/);
    }
  } finally {
    await t.cleanup();
  }
});
