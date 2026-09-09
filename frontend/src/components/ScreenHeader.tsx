export function ScreenHeader({ title, sub }: { title: string; sub?: string }) {
  return (
    <div className="screen-header">
      <h1>{title}</h1>
      {sub !== undefined && <span className="screen-header-sub">{sub}</span>}
    </div>
  );
}
