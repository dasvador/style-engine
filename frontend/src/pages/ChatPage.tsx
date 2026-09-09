import { useEffect, useRef, useState } from 'react';
import { api } from '../api/endpoints';
import { errorMessage } from '../api/client';
import type { ChatItem, Gender, StyleMood, WeatherResponse } from '../types/api';
import { ScreenHeader } from '../components/ScreenHeader';
import { LookbookCard } from '../components/LookbookCard';

/**
 * 대화 한 줄.
 *
 * AI 응답은 두 형태로 갈린다 — 착장(items)이 있으면 룩북 카드, 없으면 말풍선.
 * 문자열로 HTML 을 만들지 않기 위해 종류를 타입으로 구분한다.
 */
type Message =
  | { kind: 'user'; id: number; text: string }
  | { kind: 'ai-text'; id: number; text: string }
  | { kind: 'ai-lookbook'; id: number; reply: string; items: ChatItem[] }
  | { kind: 'error'; id: number; text: string };

interface Props {
  weather: WeatherResponse | null;
  gender: Gender;
  moods: StyleMood[];
  selectedMood: string | null;
}

export function ChatPage({ weather, gender, moods, selectedMood }: Props) {
  const [messages, setMessages] = useState<Message[]>([]);
  const [input, setInput] = useState('');
  const [sending, setSending] = useState(false);
  const containerRef = useRef<HTMLDivElement>(null);
  const nextId = useRef(1);

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

    try {
      const r = await api.chat.send({ message: text, gender, style_mood: selectedMood });
      const id = nextId.current++;
      setMessages((prev) => [
        ...prev,
        r.items.length > 0
          ? { kind: 'ai-lookbook', id, reply: r.reply, items: r.items }
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
        <span className="chat-mood-value">{moodLabel}</span>
      </div>

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
                  selectedMood={selectedMood}
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
