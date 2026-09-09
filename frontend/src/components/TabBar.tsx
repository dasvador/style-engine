import { NavLink } from 'react-router-dom';

/** 하단 내비게이션. 경로는 해시 기반에서 일반 경로로 바뀌었다. */
const TABS = [
  {
    to: '/',
    label: '홈',
    icon: (
      <>
        <path d="M3 9l9-7 9 7v11a2 2 0 01-2 2H5a2 2 0 01-2-2z" />
        <polyline points="9 22 9 12 15 12 15 22" />
      </>
    ),
  },
  {
    to: '/chat',
    label: '상담',
    icon: <path d="M21 15a2 2 0 01-2 2H7l-4 4V5a2 2 0 012-2h14a2 2 0 012 2z" />,
  },
  {
    to: '/evaluate',
    label: '평가',
    icon: (
      <>
        <path d="M9 12l2 2 4-4" />
        <circle cx="12" cy="12" r="10" />
      </>
    ),
  },
  {
    to: '/wardrobe',
    label: '옷장',
    icon: (
      <path d="M20 7h-4V4a1 1 0 00-1-1H9a1 1 0 00-1 1v3H4a1 1 0 00-1 1v12a1 1 0 001 1h16a1 1 0 001-1V8a1 1 0 00-1-1zM10 5h4v2h-4V5z" />
    ),
  },
];

export function TabBar() {
  return (
    <nav className="tab-bar">
      {TABS.map((t) => (
        <NavLink
          key={t.to}
          to={t.to}
          end={t.to === '/'}
          className={({ isActive }) => `tab-item${isActive ? ' active' : ''}`}
        >
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
            {t.icon}
          </svg>
          <span>{t.label}</span>
        </NavLink>
      ))}
    </nav>
  );
}
