// 이미 생성해 둔 룩북 사진을 데모 asset 으로 가져온다(로컬 전용, 결과는 gitignore 된 mcp/.assets/).
//
//   node scripts/import-image-assets.mjs [--sources .assets/sources.json] [--static-dir ../static/images] [--out .assets]
//
// sources.json(로컬, 저장소에 넣지 않음):
//   { "assets": { "<asset key>": { "static_file": "<uuid>.png", "source": { 근거 메모 } } } }
// asset key 는 src/photo-sets.js 의 키여야 한다. static_file 은 static/images 바로 아래의 생성 결과 파일
// 이름(UUID.png)만 받는다 — 다른 경로·심볼릭 링크·사용자 업로드는 받지 않는다.
//
// 원본 PNG 를 **바이트 그대로** 복사한다. 줄이거나 다시 인코딩하거나 메타데이터를 지우지 않는다 —
// 생성 원본에 붙은 C2PA 출처 정보(caBX 청크)를 그대로 보존해 내보낸다. 복사본의 sha256 이 원본과
// 같은지 확인하고, 파일 이름만 해시 이름으로 바꾼다(원본 경로·UUID 는 밖으로 나가지 않는다).
// 새 생성·외부 호출은 하지 않는다.

import { createHash } from 'node:crypto';
import { lstat, mkdir, readFile, readdir, rm, writeFile } from 'node:fs/promises';
import { join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

import { MAX_ASSET_BYTES, sniffMime } from '../src/image-assets.js';
import { PHOTO_ASSET_KEYS } from '../src/photo-sets.js';

const pkg = resolve(fileURLToPath(new URL('..', import.meta.url)));
const arg = (name, fallback) => {
  const i = process.argv.indexOf(name);
  return i > 0 ? resolve(process.argv[i + 1]) : fallback;
};

const STATIC_NAME = /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}\.png$/;

/** PNG 청크 이름 목록(순서대로). PNG 가 아니면 null. */
export function pngChunks(buf) {
  if (sniffMime(buf) !== 'image/png') return null;
  const types = [];
  let i = 8;
  while (i + 12 <= buf.length) {
    const len = buf.readUInt32BE(i);
    const type = buf.toString('ascii', i + 4, i + 8);
    types.push(type);
    i += 12 + len;
    if (type === 'IEND') break;
  }
  return types;
}

export function pngSize(buf) {
  return sniffMime(buf) === 'image/png' ? { width: buf.readUInt32BE(16), height: buf.readUInt32BE(20) } : null;
}

const sha256Of = (buf) => createHash('sha256').update(buf).digest('hex');

async function main() {
  const sourcesFile = arg('--sources', join(pkg, '.assets/sources.json'));
  const staticDir = arg('--static-dir', resolve(pkg, '../static/images'));
  const outDir = arg('--out', join(pkg, '.assets'));

  const sources = JSON.parse(await readFile(sourcesFile, 'utf8'));
  const assets = {};
  await mkdir(join(outDir, 'images'), { recursive: true });
  for (const [key, entry] of Object.entries(sources.assets ?? {})) {
    if (!PHOTO_ASSET_KEYS.includes(key)) throw new Error(`photo-sets.js 에 없는 asset key: ${key}`);
    const file = entry.static_file;
    if (typeof file !== 'string' || !STATIC_NAME.test(file)) throw new Error(`${key}: static_file 은 UUID.png 이름만`);
    const src = join(staticDir, file);
    const st = await lstat(src);
    if (!st.isFile() || st.isSymbolicLink()) throw new Error(`${key}: 일반 파일이 아님`);
    if (st.size > MAX_ASSET_BYTES) throw new Error(`${key}: ${MAX_ASSET_BYTES} 바이트 상한을 넘음`);
    const original = await readFile(src);
    if (sniffMime(original) !== 'image/png') throw new Error(`${key}: PNG 가 아님`);
    const size = pngSize(original);
    const chunks = pngChunks(original);

    const sha256 = sha256Of(original);
    const name = `${sha256.slice(0, 32)}.png`;
    const dest = join(outDir, 'images', name);
    await writeFile(dest, original, { mode: 0o644 });
    // 바이트 그대로인지 다시 읽어 확인한다.
    if (sha256Of(await readFile(dest)) !== sha256) throw new Error(`${key}: 복사본이 원본과 다름`);

    assets[key] = {
      file: name,
      sha256,
      bytes: original.length,
      ...size,
      mime: 'image/png',
      provenance: { byte_identical_to_original: true, c2pa_chunk_present: chunks.includes('caBX') },
      source: { ...(entry.source ?? {}), static_file: file, original_sha256: sha256 },
    };
    console.error(
      `${key} → images/${name} (${size.width}x${size.height}, ${(original.length / 1024).toFixed(0)}KB, 원본 그대로, C2PA ${chunks.includes('caBX') ? '있음' : '없음'})`,
    );
  }
  // manifest 에 없는 옛 파일(예: 예전 축소 JPEG)은 지운다 — 허용 목록과 디렉터리를 같게.
  const keep = new Set(Object.values(assets).map((a) => a.file));
  for (const f of await readdir(join(outDir, 'images'))) {
    if (!keep.has(f)) {
      await rm(join(outDir, 'images', f), { force: true });
      console.error(`지움: images/${f}`);
    }
  }
  await writeFile(
    join(outDir, 'manifest.json'),
    JSON.stringify({ version: 1, generated_at: new Date().toISOString(), assets }, null, 2),
  );
  console.error(`manifest.json: asset ${Object.keys(assets).length}개`);
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  main().catch((e) => {
    console.error(`가져오기 실패: ${e.message}`);
    process.exit(1);
  });
}
