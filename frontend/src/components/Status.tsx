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

export function EmptyState({ icon, text }: { icon: string; text: string }) {
  return (
    <div className="empty-state">
      <div className="empty-state-icon">{icon}</div>
      <div className="empty-state-text">{text}</div>
    </div>
  );
}
