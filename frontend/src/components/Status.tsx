/** 로딩 / 빈 결과 / 오류 — 화면마다 반복되던 표현을 한곳에 모았다. */

interface LoadingProps {
  children?: React.ReactNode;
  className?: string;
}

export function Loading({ children = '불러오는 중...', className }: LoadingProps) {
  return (
    <div className={className ?? 'msg msg-loading'}>
      <span className="spinner" /> {children}
    </div>
  );
}

export function ErrorMessage({ children, onRetry }: { children: React.ReactNode; onRetry?: () => void }) {
  return (
    <div className="msg msg-error" onClick={onRetry} style={onRetry ? { cursor: 'pointer' } : undefined}>
      {children}
    </div>
  );
}

/**
 * 빈 화면. 큰 이모지 하나만 두면 미완성처럼 보여서, 지면 머리글처럼
 * 작은 표식과 한 문장을 둔다. `icon` 은 호출부 호환을 위해 남겨 두고
 * 표식 자리에 쓴다.
 */
export function EmptyState({ icon, text }: { icon: string; text: string }) {
  return (
    <div className="empty-state">
      <div className="empty-state-mark">{icon}</div>
      <p className="empty-state-text">{text}</p>
    </div>
  );
}
