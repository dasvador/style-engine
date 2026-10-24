// 계정 연결 코드 발급(서버 운영자 = 계정 소유자 전용, 로컬 터미널에서).
//
//   LIVE_DATABASE_URL=... npm run link-code -- --user-id <Wardrobe Edit 사용자 id> [--ttl-min 10]
//   기존 default 옷장(토큰 없이 쓰던 로컬 데이터)에 연결하려면 --allow-default 를 명시해야 한다.
//
// 코드 원문은 이 터미널에 한 번만 나온다(파일에는 해시만). 10분 안에, 한 번만 쓸 수 있다.
// 코드를 화면 공유·채팅·로그에 남기지 말 것. 접속 문자열은 출력하지 않는다.

import { resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

import { LiveAdapter } from '../src/adapters/live.js';
import { issueLinkCode } from '../src/auth/link-codes.js';

const pkg = resolve(fileURLToPath(new URL('..', import.meta.url)));
const argv = process.argv.slice(2);
const opt = (name) => {
  const i = argv.indexOf(name);
  return i >= 0 ? argv[i + 1] : undefined;
};

async function main() {
  const userId = opt('--user-id');
  const ttlMin = Number(opt('--ttl-min') ?? 10);
  if (!userId) throw new Error('--user-id 가 필요해요.');
  if (userId === 'default' && !argv.includes('--allow-default')) {
    throw new Error('default(기존 로컬 옷장)에 연결하려면 --allow-default 를 명시하세요.');
  }
  if (!process.env.LIVE_DATABASE_URL) throw new Error('LIVE_DATABASE_URL 이 필요해요(계정이 있는지 확인).');
  const adapter = await LiveAdapter.connect(process.env.LIVE_DATABASE_URL);
  try {
    if (!(await adapter.exists(userId))) throw new Error('그런 사용자가 없어요.');
  } finally {
    await adapter.close();
  }
  const file = process.env.LINK_CODES_FILE ? resolve(process.env.LINK_CODES_FILE) : resolve(pkg, '.data/link-codes.json');
  const { code, expiresAt } = await issueLinkCode({ file, userId, ttlMs: Math.round(ttlMin * 60_000) });
  console.log(`계정 연결 코드: ${code}`);
  console.log(`만료: ${new Date(expiresAt).toLocaleString('ko-KR')} (한 번만 사용)`);
  console.log('ChatGPT 의 Wardrobe Edit 연결 화면에 입력하세요. 다른 곳에 붙여 넣거나 저장하지 마세요.');
}

main().catch((e) => {
  console.error(`발급 실패: ${e.message}`);
  process.exit(1);
});
