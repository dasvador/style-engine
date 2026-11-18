import type { LookSide } from '../hooks/useLookSide';

/**
 * 사진 위 모서리의 원본/옆모습 전환과, 만드는 중 표시. 사진 누르기와 겹치지 않는 별도 버튼이다.
 * 부모는 `position: relative` 여야 한다. 오류 문구는 `SideError` 로 사진 밖에 따로 둔다.
 */
export function SideToggle({ side }: { side: LookSide }) {
  return (
    <>
      <div className="saved-angle" role="group" aria-label="사진 각도">
        <button
          type="button"
          className={side.view === 'original' ? 'on' : ''}
          aria-pressed={side.view === 'original'}
          onClick={side.showOriginal}
        >
          원본
        </button>
        <button
          type="button"
          className={side.view === 'side' ? 'on' : ''}
          aria-pressed={side.view === 'side'}
          disabled={side.making}
          onClick={side.showSide}
          title={side.ready ? undefined : '처음 누를 때 한 장 만들어요'}
        >
          옆모습
        </button>
      </div>
      {side.making && (
        <span className="saved-angle-busy" aria-live="polite">
          <span className="spinner" /> 옆모습 만드는 중…
        </span>
      )}
    </>
  );
}

export function SideError({ side }: { side: LookSide }) {
  if (!side.error) return null;
  return (
    <span className="saved-angle-error" role="alert">
      {side.error}
      {side.canRetry && (
        <button type="button" className="link-action" onClick={side.showSide}>
          다시 시도
        </button>
      )}
    </span>
  );
}
