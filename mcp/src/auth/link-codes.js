// 계정 연결 코드 — Wardrobe Edit 계정을 가진 사람임을 증명하는 짧은 수명의 일회용 코드.
//
// 외부 로그인(IdP)이 아직 없어서, 서버를 운영하는 사람(= 이 컴퓨터와 DB 에 접근할 수 있는 계정 소유자)이
// `npm run link-code -- --user-id <id>` 로 코드를 만들고, ChatGPT 의 연결 화면(동의 화면)에 입력한다.
// 그러면 그 ChatGPT 연결이 그 계정에 묶인다. 사용자 id 를 화면이나 도구 인자로 받지 않는다 — 코드가 정한다.
//
//   - 코드: 20자 Crockford base32(약 100비트), 4자씩 끊어 보여 준다. 대소문자·하이픈·공백 무시.
//   - 저장: sha256 해시만(mcp/.data/link-codes.json, gitignore). 원문은 발급한 터미널에 한 번만 나온다.
//   - 수명: 기본 10분, 최대 60분. 한 번 쓰면 끝(서버의 auth-store 에 소모 기록).
// 이 파일은 CLI 가 쓰고 서버는 읽기만 한다(소모 기록은 서버 자신의 저장소에).

import { randomBytes } from 'node:crypto';
import { mkdir, readFile, rename, writeFile } from 'node:fs/promises';
import { dirname } from 'node:path';

import { sha256 } from './store.js';

const ALPHABET = '0123456789ABCDEFGHJKMNPQRSTVWXYZ'; // Crockford base32
export const LINK_CODE_LENGTH = 20;
export const DEFAULT_TTL_MS = 10 * 60 * 1000;
export const MAX_TTL_MS = 60 * 60 * 1000;

export function generateLinkCode() {
  const bytes = randomBytes(LINK_CODE_LENGTH);
  const chars = [...bytes].map((b) => ALPHABET[b & 31]).join('');
  return chars.match(/.{4}/g).join('-');
}

/** 입력을 비교용 모양으로. 모양이 아니면 null(조회하지 않는다). */
export function normalizeLinkCode(input) {
  if (typeof input !== 'string' || input.length > 64) return null;
  const s = input
    .toUpperCase()
    .replace(/[\s-]/g, '')
    .replace(/O/g, '0')
    .replace(/[IL]/g, '1');
  return new RegExp(`^[${ALPHABET}]{${LINK_CODE_LENGTH}}$`).test(s) ? s : null;
}

export const linkCodeHash = (normalized) => sha256(`wardrobe-link:${normalized}`);

async function readCodes(file) {
  try {
    const parsed = JSON.parse(await readFile(file, 'utf8'));
    return Array.isArray(parsed?.codes) ? parsed : { version: 1, codes: [] };
  } catch (e) {
    if (e.code === 'ENOENT') return { version: 1, codes: [] };
    throw e;
  }
}

/** 새 코드를 만들어 해시만 파일에 남기고 원문을 돌려준다(CLI 용). */
export async function issueLinkCode({ file, userId, ttlMs = DEFAULT_TTL_MS, now = Date.now() }) {
  if (!Number.isInteger(ttlMs) || ttlMs < 60_000 || ttlMs > MAX_TTL_MS) throw new Error('수명은 1~60분이에요.');
  const code = generateLinkCode();
  const data = await readCodes(file);
  data.codes = data.codes.filter((c) => c.expires_at > now); // 만료된 것 정리
  data.codes.push({
    hash: linkCodeHash(normalizeLinkCode(code)),
    user_id: userId,
    created_at: now,
    expires_at: now + ttlMs,
  });
  await mkdir(dirname(file), { recursive: true });
  const tmp = `${file}.${process.pid}.tmp`;
  await writeFile(tmp, JSON.stringify(data, null, 2), { mode: 0o600 });
  await rename(tmp, file);
  return { code, expiresAt: now + ttlMs };
}

/** 입력한 코드의 항목(아직 만료 전). 소모 여부는 서버 저장소가 따로 본다. */
export async function findLinkCode({ file, input, now = Date.now() }) {
  const normalized = normalizeLinkCode(input);
  if (!normalized) return null;
  const hash = linkCodeHash(normalized);
  const data = await readCodes(file);
  const entry = data.codes.find((c) => c.hash === hash);
  if (!entry || entry.expires_at <= now) return null;
  return { hash, userId: entry.user_id, expiresAt: entry.expires_at };
}
