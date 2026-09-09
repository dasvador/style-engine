import { useCallback, useEffect, useState } from 'react';
import { Route, Routes } from 'react-router-dom';
import { api } from './api/endpoints';
import { useAsync } from './hooks/useAsync';
import { useClothes } from './hooks/useClothes';
import type { Gender, Region, StyleMood, WeatherResponse } from './types/api';
import { Layout } from './components/Layout';
import { RegionModal } from './components/RegionModal';
import { HomePage } from './pages/HomePage';
import { ChatPage } from './pages/ChatPage';
import { EvaluatePage } from './pages/EvaluatePage';
import { WardrobePage } from './pages/WardrobePage';
import { ItemDetailPage } from './pages/ItemDetailPage';

/**
 * 앱 셸.
 *
 * 옷장·날씨·지역·무드는 여러 화면이 공유하므로 여기서 한 번 읽어 내려보낸다.
 * 전역 상태 라이브러리를 쓰지 않은 이유: 공유 대상이 넷뿐이고 갱신 시점이 명확해서,
 * prop 으로 내리는 편이 흐름을 읽기 쉽다.
 */
export function App() {
  const clothes = useClothes();
  const weather = useAsync<WeatherResponse>(() => api.weather(), []);
  const region = useAsync<Region>(() => api.region.get(), []);

  // 여성 기준으로 먼저 보여준다. 이미지 생성도 여성 모델을 전제로 하고 있어
  // 첫 화면과 생성 결과가 어긋나지 않는다.
  const [gender, setGender] = useState<Gender>('female');
  const [moods, setMoods] = useState<StyleMood[]>([]);
  const [selectedMood, setSelectedMood] = useState<string | null>(null);
  const [regionOpen, setRegionOpen] = useState(false);
  const [addPanelOpen, setAddPanelOpen] = useState(false);

  // 성별이 바뀌면 무드 목록을 다시 읽고 첫 항목을 고른다 (기존 동작과 동일).
  useEffect(() => {
    let alive = true;
    void (async () => {
      try {
        const list = await api.moods(gender);
        if (!alive) return;
        setMoods(list);
        setSelectedMood(list[0]?.mood_key ?? null);
      } catch {
        if (alive) {
          setMoods([]);
          setSelectedMood(null);
        }
      }
    })();
    return () => {
      alive = false;
    };
  }, [gender]);

  const reloadWeather = useCallback(() => {
    void weather.reload();
    void region.reload();
  }, [weather, region]);

  return (
    <>
      <Routes>
        <Route element={<Layout />}>
          <Route
            index
            element={
              <HomePage
                clothes={clothes.items}
                weather={weather.data}
                weatherLoading={weather.loading}
                weatherError={weather.error}
                region={region.data}
                moods={moods}
                gender={gender}
                onGenderChange={setGender}
                selectedMood={selectedMood}
                onMoodChange={setSelectedMood}
                onOpenRegion={() => setRegionOpen(true)}
                onOpenAddPanel={() => setAddPanelOpen(true)}
              />
            }
          />
          <Route
            path="chat"
            element={
              <ChatPage weather={weather.data} gender={gender} moods={moods} selectedMood={selectedMood} />
            }
          />
          <Route path="evaluate" element={<EvaluatePage clothes={clothes.items} />} />
          <Route
            path="wardrobe"
            element={
              <WardrobePage
                clothes={clothes.items}
                onReload={clothes.reload}
                addPanelOpen={addPanelOpen}
                setAddPanelOpen={setAddPanelOpen}
              />
            }
          />
          <Route
            path="wardrobe/:id"
            element={<ItemDetailPage clothes={clothes.items} onReload={clothes.reload} />}
          />
          {/* 알 수 없는 경로는 홈으로 (SPA fallback 이 index.html 을 주므로 여기서 처리) */}
          <Route
            path="*"
            element={
              <HomePage
                clothes={clothes.items}
                weather={weather.data}
                weatherLoading={weather.loading}
                weatherError={weather.error}
                region={region.data}
                moods={moods}
                gender={gender}
                onGenderChange={setGender}
                selectedMood={selectedMood}
                onMoodChange={setSelectedMood}
                onOpenRegion={() => setRegionOpen(true)}
                onOpenAddPanel={() => setAddPanelOpen(true)}
              />
            }
          />
        </Route>
      </Routes>

      <RegionModal
        open={regionOpen}
        region={region.data}
        onClose={() => setRegionOpen(false)}
        onSaved={reloadWeather}
      />
    </>
  );
}
