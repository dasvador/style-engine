import type { StyleMood } from "../types/api";

interface Props {
  moods: StyleMood[];
  selectedMood: string | null;
  onMoodChange: (key: string) => void;
}

/** 스타일 무드 선택 칩. 홈과 상담이 같은 것을 쓴다 — 고른 무드는 두 화면이 공유한다. */
export function MoodChips({ moods, selectedMood, onMoodChange }: Props) {
  return (
    <div className="mood-chips" role="group" aria-label="스타일 장르 선택">
      {moods.map((m) => (
        <button
          key={m.mood_key}
          className={`mood-chip-btn${selectedMood === m.mood_key ? " active" : ""}`}
          aria-pressed={selectedMood === m.mood_key}
          onClick={() => onMoodChange(m.mood_key)}
        >
          {m.mood_label}
        </button>
      ))}
    </div>
  );
}
