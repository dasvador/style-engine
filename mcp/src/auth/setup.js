// live 모드 조립: 저장소 + provider + express 앱(OAuth 경로·Bearer 검사).

import { getOAuthProtectedResourceMetadataUrl } from '@modelcontextprotocol/sdk/server/auth/router.js';

import { AuthStore } from './store.js';
import { SCOPE_COMPARISONS_WRITE, SCOPE_READ, createAuthApp, createOAuthProvider } from './oauth.js';

/**
 * @param {{ origin: string, authStoreFile: string|null, linkCodesFile: string, redirectAllowlist: string[],
 *           adapter: { exists(id: string): Promise<boolean> } & object, now?: () => number }} opts
 *   origin: 공개 origin(https://PUBLIC_HOST). 발급자(issuer)이자 리소스(/mcp)의 기준. 요청 헤더로 만들지 않는다.
 */
export async function createLiveAuth({ origin, authStoreFile, linkCodesFile, redirectAllowlist, adapter, now }) {
  const issuer = new URL(origin);
  const resource = new URL('/mcp', origin);
  const store = await new AuthStore({ file: authStoreFile, now }).load();
  const provider = createOAuthProvider({
    store,
    issuer,
    resource,
    redirectAllowlist,
    linkCodesFile,
    users: { exists: (id) => adapter.exists(id) },
  });
  return {
    store,
    provider,
    issuer,
    resource,
    ownOrigin: issuer.origin,
    live: {
      adapter,
      resourceMetadataUrl: getOAuthProtectedResourceMetadataUrl(resource),
      scopes: { read: SCOPE_READ, write: SCOPE_COMPARISONS_WRITE },
    },
    makeApp: (onMcp) => createAuthApp({ provider, issuer, resource, ownOrigin: issuer.origin, onMcp }),
  };
}
