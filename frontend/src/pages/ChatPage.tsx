import { useEffect, useRef, useState } from 'react';
import { api } from '../api/endpoints';
import { errorMessage } from '../api/client';
import type { ChatHistoryRow, ChatItem, Gender, StyleMood, WeatherResponse } from '../types/api';
import { ScreenHeader } from '../components/ScreenHeader';
import { LookbookCard } from '../components/LookbookCard';
import { MoodChips } from '../components/MoodChips';

/**
 * 대화 한 줄.
 *
 * AI 응답은 두 형태로 갈린다 — 착장(items)이 있으면 룩북 카드, 없으면 말풍선.
 * 문자열로 HTML 을 만들지 않기 위해 종류를 타입으로 구분한다.
 */
type Message =
  | { kind: 'user'; id: number; text: string }
  | { kind: 'ai-text'; id: number; text: string }
  | {
      kind: 'ai-lookbook';
      id: number;
      reply: string;
      items: ChatItem[];
      baseItems?: ChatItem[];
      /** 이 카드를 만들 때의 무드. 사진 캐시 키에 들어가므로 화면의 현재 무드로 그리면
       *  무드를 바꿀 때마다 예전 카드 사진을 새로 만든다. */
      mood: string | null;
    }
  | { kind: 'error'; id: number; text: string };

interface Props {
  weather: WeatherResponse | null;
  gender: Gender;
  moods: StyleMood[];
  selectedMood: string | null;
  onMoodChange: (key: string) => void;
}

/** 서버에 남은 한 줄을 화면의 한 줄로. id 는 서버 값을 음수로 써서 새 메시지와 겹치지 않게 한다. */
function fromHistory(row: ChatHistoryRow): Message {
  const id = -row.id;
  if (row.role === 'user') return { kind: 'user', id, text: row.text };
  if (row.items_json && row.items_json.length > 0) {
    return {
      kind: 'ai-lookbook',
      id,
      reply: row.text,
      items: row.items_json,
      baseItems: row.base_items_json ?? undefined,
      mood: row.mood_key,
    };
  }
  return { kind: 'ai-text', id, text: row.text || '추천 결과가 없습니다.' };
}

export function ChatPage({ weather, gender, moods, selectedMood, onMoodChange }: Props) {
  const [messages, setMessages] = useState<Message[]>([]);
  const [input, setInput] = useState('');
  const [sending, setSending] = useState(false);
  const [confirmingClear, setConfirmingClear] = useState(false);
  const [pickingMood, setPickingMood] = useState(false);
  const containerRef = useRef<HTMLDivElement>(null);
  const nextId = useRef(1);

  // 지난 대화를 불러온다. 사진은 코디별로 저장돼 있어 다시 열어도 새로 만들지 않는다.
  // 불러오는 사이에 보낸 메시지가 있으면 그 뒤에 이어 붙는다.
  useEffect(() => {
    let alive = true;
    api.chat
      .history()
      .then((rows) => {
        if (!alive) return;
        setMessages((prev) => [...rows.map(fromHistory), ...prev]);
      })
      .catch(() => {
        // 기록을 못 불러와도 새 대화는 할 수 있다.
      });
    return () => {
      alive = false;
    };
  }, []);

  const clearHistory = async () => {
    try {
      await api.chat.clearHistory();
      setMessages([]);
    } catch (err) {
      setMessages((prev) => [
        ...prev,
        { kind: 'error', id: nextId.current++, text: `대화를 지우지 못했어요: ${errorMessage(err)}` },
      ]);
    } finally {
      setConfirmingClear(false);
    }
  };

  const moodLabel = moods.find((m) => m.mood_key === selectedMood)?.mood_label ?? '전체';

  useEffect(() => {
    containerRef.current?.scrollTo({ top: containerRef.current.scrollHeight });
  }, [messages, sending]);

  const send = async (e: React.FormEvent) => {
    e.preventDefault();
    const text = input.trim();
    if (!text || sending) return;

    const userId = nextId.current++;
    setMessages((prev) => [...prev, { kind: 'user', id: userId, text }]);
    setInput('');
    setSending(true);

    // 바로 앞 코디. 상담은 대화를 서버에 남기지 않아서, "신발만 바꿔줘" 가 무엇을
    // 바꾸라는 말인지 알려면 화면이 넘겨야 한다. 같은 코디로 앞 사진을 편집하는 데도 쓴다.
    const previous = [...messages].reverse().find((m) => m.kind === 'ai-lookbook');
    const previousItems = previous?.kind === 'ai-lookbook' ? previous.items : undefined;

    try {
      const r = await api.chat.send({
        message: text,
        gender,
        style_mood: selectedMood,
        previous_items: previousItems,
      });
      const id = nextId.current++;
      setMessages((prev) => [
        ...prev,
        r.items.length > 0
          ? {
              kind: 'ai-lookbook',
              id,
              reply: r.reply,
              items: r.items,
              baseItems: previousItems,
              mood: selectedMood,
            }
          : { kind: 'ai-text', id, text: r.reply || '추천 결과가 없습니다.' },
      ]);
    } catch (err) {
      setMessages((prev) => [
        ...prev,
        { kind: 'error', id: nextId.current++, text: `오류가 발생했어요: ${errorMessage(err)}` },
      ]);
    } finally {
      setSending(false);
    }
  };

  return (
    <>
      <ScreenHeader title="스타일 상담" sub="옷장을 두고 나누는 대화" />

      <div className="chat-mood-line">
        <span>현재 무드</span>
        {/* 무드를 바꾸면 다음 질문부터 그 무드로 답한다. 이미 받은 카드는 만들 때의 무드로
            그대로 그린다 — 사진을 다시 만들지 않는다. */}
        <button
          className="chat-mood-value chat-mood-toggle"
          aria-expanded={pickingMood}
          onClick={() => setPickingMood((v) => !v)}
        >
          {moodLabel} <span aria-hidden="true">{pickingMood ? '▴' : '▾'}</span>
        </button>
        {messages.length > 0 && (
          <span className="saved-delete chat-clear">
            {confirmingClear ? (
              <>
                <span className="saved-delete-ask">대화를 모두 지울까요?</span>
                <button className="saved-delete-yes" onClick={() => void clearHistory()}>
                  지우기
                </button>
                <button className="saved-delete-no" onClick={() => setConfirmingClear(false)}>
                  취소
                </button>
              </>
            ) : (
              <button className="saved-delete-btn" onClick={() => setConfirmingClear(true)}>
                대화 지우기
              </button>
            )}
          </span>
        )}
      </div>
      {pickingMood && (
        <div className="chat-mood-picker">
          <MoodChips
            moods={moods}
            selectedMood={selectedMood}
            onMoodChange={(key) => {
              onMoodChange(key);
              setPickingMood(false);
            }}
          />
        </div>
      )}

      <div className="chat-container" ref={containerRef}>
        <div className="chat-bubble chat-ai">
          옷장에 있는 아이템으로 코디를 함께 골라 드려요.
          <br />
          <span className="chat-hint">예: &quot;네이비 스니커에 맞는 상하의 추천해줘&quot;</span>
        </div>

        {messages.map((m) => {
          switch (m.kind) {
            case 'user':
              return (
                <div className="chat-bubble chat-user" key={m.id}>
                  {m.text}
                </div>
              );
            case 'ai-text':
              // LLM 문자열 — 줄바꿈만 살리고 텍스트로 렌더링한다.
              return (
                <div className="chat-bubble chat-ai" key={m.id} style={{ whiteSpace: 'pre-line' }}>
                  {m.text}
                </div>
              );
            case 'ai-lookbook':
              return (
                <LookbookCard
                  key={m.id}
                  items={m.items}
                  reply={m.reply}
                  weather={weather?.current ?? null}
                  selectedMood={m.mood}
                  baseItems={m.baseItems}
                />
              );
            case 'error':
              return (
                <div className="chat-bubble chat-ai chat-error" key={m.id}>
                  {m.text}
                </div>
              );
          }
        })}

        {sending && <div className="chat-bubble chat-ai chat-typing">생각하는 중...</div>}
      </div>

      <form className="chat-input-bar" onSubmit={send}>
        <input
          type="text"
          className="chat-input"
          placeholder="어떤 코디가 궁금하세요?"
          autoComplete="off"
          value={input}
          onChange={(e) => setInput(e.target.value)}
        />
        <button type="submit" className="chat-send-btn" disabled={sending}>
          <svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.5">
            <path d="M22 2L11 13" />
            <path d="M22 2l-7 20-4-9-9-4 20-7z" />
          </svg>
        </button>
      </form>
    </>
  );
}
