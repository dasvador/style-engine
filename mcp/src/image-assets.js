// 로컬 이미지 asset 허용 목록. 저장소에 넣지 않는 `mcp/.assets/` 의 manifest 와 파일만 읽는다.
//
// manifest.json(version 1, scripts/import-image-assets.mjs 가 만든다):
//   { "version": 1, "assets": { "<asset key>": { "file": "<sha256 앞 32자>.<jpg|png>", "sha256": "...",
//     "bytes": n, "width": w, "height": h, "mime": "image/png",
//     "provenance": { "byte_identical_to_original": true, "c2pa_chunk_present": true },
//     "source": { ... 로컬 근거 ... } } } }
// 사진은 생성 원본 PNG 를 바이트 그대로 복사한 것이다(C2PA 출처 정보 보존, 축소·재인코딩 없음).
// source 는 이 파일 안에만 있고 화면·도구 결과로 내보내지 않는다.
//
// 시작할 때 한 번 검사한다: 이름 모양, 같은 디렉터리 안의 일반 파일(심볼릭 링크 거절), 크기, 서명
// (PNG/JPEG 매직 바이트로 mime 확인), sha256. 하나라도 어긋난 asset 은 허용 목록에 넣지 않는다 —
// 그 asset 을 쓰는 사진 세트는 나오지 않는다. 요청 경로로 파일 시스템을 찾지 않는다(이름 → Map 조회만).

import { createHash } from 'node:crypto';
import { lstat, readFile, realpath } from 'node:fs/promises';
import { join, sep } from 'node:path';

/** 서버가 내보내는 파일 이름 모양. 해시 이름이라 내용이 바뀌면 이름도 바뀐다(오래 캐시해도 된다). */
export const ASSET_NAME_PATTERN = /^[0-9a-f]{32}\.(jpg|png)$/;
export const ASSET_ROUTE_PREFIX = '/assets/img/';
/** asset 한 장 상한. 생성 원본 PNG(약 2.1~2.3MB)를 바이트 그대로 내보내므로 넉넉히 둔다. */
export const MAX_ASSET_BYTES = 8 * 1024 * 1024;

export function sniffMime(buf) {
  if (buf.length >= 8 && buf.subarray(0, 8).equals(Buffer.from([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]))) {
    return 'image/png';
  }
  if (buf.length >= 3 && buf[0] === 0xff && buf[1] === 0xd8 && buf[2] === 0xff) return 'image/jpeg';
  return null;
}

const EXT_MIME = { png: 'image/png', jpg: 'image/jpeg' };

/**
 * @typedef {{ key: string, name: string, path: string, mime: string, bytes: number, width: number,
 *             height: number, etag: string, data: Buffer }} LoadedAsset
 * @returns {Promise<{ byKey: Map<string, LoadedAsset>, byName: Map<string, LoadedAsset>, problems: string[] }>}
 */
export async function loadImageAssets(dir) {
  const byKey = new Map();
  const byName = new Map();
  const problems = [];
  if (!dir) return { byKey, byName, problems };

  let manifest;
  try {
    manifest = JSON.parse(await readFile(join(dir, 'manifest.json'), 'utf8'));
  } catch (e) {
    if (e.code !== 'ENOENT') problems.push(`manifest.json 을 읽지 못함(${e.code ?? 'parse'})`);
    return { byKey, byName, problems };
  }
  if (manifest?.version !== 1 || typeof manifest.assets !== 'object' || manifest.assets === null) {
    problems.push('manifest.json 형식이 version 1 이 아님');
    return { byKey, byName, problems };
  }

  let root;
  try {
    root = await realpath(dir);
  } catch {
    problems.push('asset 디렉터리를 찾지 못함');
    return { byKey, byName, problems };
  }

  for (const [key, entry] of Object.entries(manifest.assets)) {
    const fail = (why) => problems.push(`${key}: ${why}`);
    const name = entry?.file;
    if (typeof name !== 'string' || !ASSET_NAME_PATTERN.test(name)) {
      fail('파일 이름 모양이 아님');
      continue;
    }
    const path = join(root, 'images', name);
    try {
      const st = await lstat(path);
      if (!st.isFile() || st.isSymbolicLink()) {
        fail('일반 파일이 아님(심볼릭 링크 거절)');
        continue;
      }
      if (st.size === 0 || st.size > MAX_ASSET_BYTES) {
        fail('크기가 범위 밖');
        continue;
      }
      const real = await realpath(path);
      if (!real.startsWith(root + sep)) {
        fail('asset 디렉터리 밖');
        continue;
      }
      const buf = await readFile(real);
      const mime = sniffMime(buf);
      const extMime = EXT_MIME[name.split('.').pop()];
      if (!mime || mime !== extMime || (entry.mime && entry.mime !== mime)) {
        fail('이미지 서명이 이름·manifest 와 다름');
        continue;
      }
      const sha256 = createHash('sha256').update(buf).digest('hex');
      if (sha256 !== entry.sha256 || !name.startsWith(sha256.slice(0, 32))) {
        fail('sha256 불일치(손상 또는 바뀐 파일)');
        continue;
      }
      const width = Number(entry.width);
      const height = Number(entry.height);
      if (!(width > 0 && height > 0)) {
        fail('width·height 없음');
        continue;
      }
      // 검사한 바로 그 바이트를 내보낸다(검사 뒤 파일이 바뀌어도 응답은 바뀌지 않는다).
      const asset = { key, name, path: real, mime, bytes: buf.length, width, height, etag: `"${sha256.slice(0, 32)}"`, data: buf };
      byKey.set(key, asset);
      byName.set(name, asset);
    } catch (e) {
      fail(e.code === 'ENOENT' ? '파일 없음' : `읽기 실패(${e.code ?? 'error'})`);
    }
  }
  return { byKey, byName, problems };
}

/**
 * 공개 origin. PUBLIC_HOST 가 있으면 늘 https://PUBLIC_HOST 이다 — 요청의 Host·X-Forwarded-* 헤더는 믿지 않는다.
 * 없으면 로컬 검사용 http://127.0.0.1:포트(ChatGPT·Claude 같은 원격 호스트에서는 보이지 않는다).
 */
export function imageOrigin({ publicHost, port }) {
  return publicHost ? `https://${publicHost.toLowerCase()}` : `http://127.0.0.1:${port}`;
}

/** asset 키 → 도구 결과에 실을 사진(절대 URL). 허용 목록에 없으면 null. */
export function imageResolver(byKey, origin) {
  return (key) => {
    const asset = byKey.get(key);
    return asset ? { url: `${origin}${ASSET_ROUTE_PREFIX}${asset.name}`, width: asset.width, height: asset.height } : null;
  };
}
