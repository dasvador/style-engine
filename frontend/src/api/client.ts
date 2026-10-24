/**
 * API 클라이언트와 공통 오류 처리.
 *
 * 항상 상대 경로(`/api/...`)로 호출한다. 개발에서는 Vite 가 프록시하고 운영에서는
 * Axum 이 같은 origin 에서 API 와 정적 파일을 모두 제공하므로, 프런트에 백엔드 주소를
 * 넣을 필요가 없다 — 즉 서버 주소나 키가 번들에 들어가지 않는다.
 */

export const API_BASE = '/api';

/**
 * 백엔드 오류 응답을 담는다.
 *
 * Axum 쪽 `AppError` 는 non-2xx 에 `{"error": "..."}` 를 실어 보낸다.
 * 이전 UI 는 오류 본문을 문자열로 던지고 호출부마다 JSON.parse 를 다시 시도했는데,
 * 그 해석을 여기 한 곳으로 모은다.
 */
export class ApiError extends Error {
  readonly status: number;
  readonly body: string;

  constructor(status: number, message: string, body: string) {
    super(message);
    this.name = 'ApiError';
    this.status = status;
    this.body = body;
  }
}

function parseErrorMessage(status: number, raw: string, statusText: string): string {
  if (raw) {
    try {
      const parsed: unknown = JSON.parse(raw);
      if (
        typeof parsed === 'object' &&
        parsed !== null &&
        'error' in parsed &&
        typeof (parsed as { error: unknown }).error === 'string'
      ) {
        return (parsed as { error: string }).error;
      }
    } catch {
      // JSON 이 아니면 본문을 그대로 쓴다.
    }
    return raw;
  }
  return statusText || `HTTP ${status}`;
}

/**
 * 계정 토큰(있으면). 백엔드는 토큰이 없거나 틀리면 401 이다(`default` 로 바꾸지 않는다).
 *
 * 아직 로그인 화면이 없다. 로컬 개발은 백엔드를 `WARDROBE_AUTH_MODE=dev-default-loopback` 으로 켜서
 * 토큰 없이 기존(default) 옷장을 쓴다. 다른 계정으로 시험하려면 `POST /api/user/register` 로 받은 토큰을
 * 브라우저 저장소의 `wardrobe.apiToken` 에 넣는다. 이 경우 `/static` 사진은 보이지 않는다(보호 경로
 * `/api/images/{file}` 은 헤더가 필요해 `<img>` 로는 못 읽는다 — 로그인 범위에서 다룬다).
 */
const TOKEN_KEY = 'wardrobe.apiToken';

function authHeaders(): Record<string, string> {
  try {
    const token = globalThis.localStorage?.getItem(TOKEN_KEY)?.trim();
    return token ? { Authorization: `Bearer ${token}` } : {};
  } catch {
    return {};
  }
}

const UNAUTHORIZED_HINT =
  '로그인이 필요해요. 로컬 개발이면 백엔드를 WARDROBE_AUTH_MODE=dev-default-loopback 으로 켜거나, 계정 토큰을 설정하세요.';

async function request<T>(path: string, init?: RequestInit): Promise<T> {
  let res: Response;
  try {
    res = await fetch(`${API_BASE}${path}`, {
      ...init,
      headers: { ...authHeaders(), ...(init?.headers as Record<string, string> | undefined) },
    });
  } catch (cause) {
    // 네트워크 단절은 상태코드가 없다. 호출부가 같은 타입으로 다룰 수 있게 감싼다.
    throw new ApiError(0, '서버에 연결할 수 없습니다.', String(cause));
  }

  if (!res.ok) {
    const raw = await res.text().catch(() => '');
    const message = res.status === 401 ? UNAUTHORIZED_HINT : parseErrorMessage(res.status, raw, res.statusText);
    throw new ApiError(res.status, message, raw);
  }

  if (res.status === 204) {
    return undefined as T;
  }

  const text = await res.text();
  if (!text) {
    return undefined as T;
  }
  return JSON.parse(text) as T;
}

const jsonHeaders = { 'Content-Type': 'application/json' };

export function apiGet<T>(path: string): Promise<T> {
  return request<T>(path);
}

export function apiPost<T>(path: string, body: unknown): Promise<T> {
  return request<T>(path, {
    method: 'POST',
    headers: jsonHeaders,
    body: JSON.stringify(body),
  });
}

export function apiPatch<T>(path: string, body: unknown): Promise<T> {
  return request<T>(path, {
    method: 'PATCH',
    headers: jsonHeaders,
    body: JSON.stringify(body),
  });
}

export function apiPut<T>(path: string, body: unknown): Promise<T> {
  return request<T>(path, {
    method: 'PUT',
    headers: jsonHeaders,
    body: JSON.stringify(body),
  });
}

export function apiDelete<T>(path: string): Promise<T> {
  return request<T>(path, { method: 'DELETE' });
}

/** 오류를 사용자에게 보여줄 한 줄로 바꾼다. */
export function errorMessage(err: unknown): string {
  if (err instanceof ApiError) return err.message;
  if (err instanceof Error) return err.message;
  return String(err);
}
