/**
 * 엔드포인트별 래퍼.
 *
 * 경로와 요청/응답 타입을 한곳에 묶어 두어, 화면 코드가 URL 문자열을 직접 다루지 않게 한다.
 * 경로는 기존 UI 가 쓰던 것과 동일하다 — 이번 전환으로 API 를 바꾸지 않았다.
 */

import { apiDelete, apiGet, apiPost, apiPut } from './client';
import type {
  ChatBody,
  ChatResponse,
  Clothing,
  CreateClothingBody,
  EvaluateBody,
  EvaluateResponse,
  FeedbackBody,
  Gender,
  HealthResponse,
  ImageUploadBody,
  MultiModeRecommendation,
  OutfitImageBody,
  OutfitImageResponse,
  RecommendationBody,
  Region,
  StyleMood,
  UpsertRegionBody,
  WeatherResponse,
} from '../types/api';

export const api = {
  health: () => apiGet<HealthResponse>('/health'),

  weather: () => apiGet<WeatherResponse>('/weather'),

  region: {
    get: () => apiGet<Region>('/region'),
    upsert: (body: UpsertRegionBody) => apiPut<Region>('/region', body),
  },

  clothes: {
    list: () => apiGet<Clothing[]>('/clothes'),
    create: (body: CreateClothingBody) => apiPost<Clothing>('/clothes', body),
    remove: (id: string) => apiDelete<{ deleted: boolean }>(`/clothes/${encodeURIComponent(id)}`),
    upload: (body: ImageUploadBody) => apiPost<Clothing>('/clothes/upload', body),
  },

  moods: (gender: Gender) =>
    apiGet<StyleMood[]>(`/style-moods?gender=${encodeURIComponent(gender)}`),

  recommendation: {
    multi: (body: RecommendationBody) =>
      apiPost<MultiModeRecommendation>('/recommendation/multi', body),
  },

  outfit: {
    evaluate: (body: EvaluateBody) => apiPost<EvaluateResponse>('/outfit/evaluate', body),
  },

  chat: {
    send: (body: ChatBody) => apiPost<ChatResponse>('/chat', body),
    image: (body: OutfitImageBody) => apiPost<OutfitImageResponse>('/chat/image', body),
  },

  feedback: (body: FeedbackBody) => apiPost<unknown>('/feedback', body),
};
