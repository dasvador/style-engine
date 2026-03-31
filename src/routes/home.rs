use axum::{response::Html, routing::get, Router};

use crate::AppState;

pub fn router() -> Router<AppState> {
    Router::new().route("/", get(home_page))
}

async fn home_page() -> Html<&'static str> {
    Html(HOME_HTML)
}

const HOME_HTML: &str = r#"<!DOCTYPE html>
<html lang="ko">
<head>
<meta charset="UTF-8">
<meta name="viewport" content="width=device-width, initial-scale=1.0">
<title>오늘뭐입지</title>
<style>
  * { margin: 0; padding: 0; box-sizing: border-box; }
  body { font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, sans-serif; background: #f5f5f5; color: #333; }
  .container { max-width: 720px; margin: 0 auto; padding: 16px; }
  header { text-align: center; padding: 24px 0 16px; }
  header h1 { font-size: 1.8rem; }
  header p { color: #888; font-size: 0.9rem; }
  .card { background: #fff; border-radius: 12px; padding: 20px; margin-bottom: 16px; box-shadow: 0 1px 3px rgba(0,0,0,0.08); }
  .card h2 { font-size: 1.1rem; margin-bottom: 12px; border-bottom: 1px solid #eee; padding-bottom: 8px; }
  .weather-grid { display: grid; grid-template-columns: repeat(3, 1fr); gap: 12px; text-align: center; }
  .weather-grid .val { font-size: 1.4rem; font-weight: 700; }
  .weather-grid .lbl { font-size: 0.75rem; color: #888; }
  .clothes-list { list-style: none; }
  .clothes-list li { display: flex; justify-content: space-between; align-items: center; padding: 8px 0; border-bottom: 1px solid #f0f0f0; }
  .clothes-list li:last-child { border-bottom: none; }
  .tag { display: inline-block; font-size: 0.7rem; background: #e8f0fe; color: #1a73e8; padding: 2px 8px; border-radius: 10px; margin-right: 4px; }
  form.inline { display: flex; gap: 8px; flex-wrap: wrap; margin-top: 12px; }
  form.inline input, form.inline select { padding: 6px 10px; border: 1px solid #ddd; border-radius: 6px; font-size: 0.85rem; }
  form.inline input[type="text"] { flex: 1; min-width: 120px; }
  button { cursor: pointer; border: none; padding: 8px 16px; border-radius: 6px; font-size: 0.85rem; font-weight: 600; }
  .btn-primary { background: #1a73e8; color: #fff; }
  .btn-primary:hover { background: #1557b0; }
  .btn-danger { background: #ea4335; color: #fff; font-size: 0.75rem; padding: 4px 10px; }
  .btn-danger:hover { background: #c5221f; }
  .btn-secondary { background: #e8e8e8; color: #333; }
  .rec-result { margin-top: 12px; white-space: pre-wrap; line-height: 1.6; }
  .outfit-item { background: #f9f9f9; border-radius: 12px; margin-top: 10px; overflow: hidden; }
  .outfit-item .cat { font-weight: 700; color: #1a73e8; }
  .outfit-item .outfit-body { padding: 10px 14px; }
  .outfit-item .outfit-img { width: 100%; max-height: 200px; object-fit: cover; display: block; }
  .outfit-grid { display: grid; grid-template-columns: repeat(auto-fit, minmax(200px, 1fr)); gap: 12px; margin-top: 12px; }
  .tips { margin-top: 8px; padding-left: 20px; }
  .tips li { margin-bottom: 4px; font-size: 0.9rem; }
  .msg { font-size: 0.85rem; color: #888; padding: 8px 0; }
  .error { color: #ea4335; }
  .loading { color: #1a73e8; }
  .upload-area { border: 2px dashed #ddd; border-radius: 8px; padding: 20px; text-align: center; margin-top: 12px; cursor: pointer; transition: border-color 0.2s, background 0.2s; }
  .upload-area:hover { border-color: #1a73e8; background: #f0f6ff; }
  .upload-area.dragging { border-color: #1a73e8; background: #e8f0fe; }
  .upload-area input[type="file"] { display: none; }
  .upload-preview { max-width: 200px; max-height: 200px; margin: 8px auto; border-radius: 8px; }
  .btn-upload { background: #34a853; color: #fff; margin-top: 8px; }
  .btn-upload:hover { background: #2d8e47; }
  .btn-upload:disabled { background: #ccc; cursor: not-allowed; }
  .modal-overlay { display: none; position: fixed; top: 0; left: 0; right: 0; bottom: 0; background: rgba(0,0,0,0.4); z-index: 100; justify-content: center; align-items: center; }
  .modal-overlay.open { display: flex; }
  .modal { background: #fff; border-radius: 12px; padding: 24px; max-width: 480px; width: 90%; max-height: 80vh; overflow-y: auto; box-shadow: 0 4px 24px rgba(0,0,0,0.15); }
  .modal h3 { font-size: 1.2rem; margin-bottom: 16px; }
  .modal-close { float: right; background: none; font-size: 1.2rem; color: #888; padding: 0 4px; }
  .modal-close:hover { color: #333; }
  .detail-row { display: flex; padding: 8px 0; border-bottom: 1px solid #f0f0f0; font-size: 0.9rem; }
  .detail-row:last-child { border-bottom: none; }
  .detail-label { width: 80px; color: #888; flex-shrink: 0; }
  .detail-value { flex: 1; }
  .clothes-list li .item-info { cursor: pointer; flex: 1; }
  .clothes-list li .item-info:hover { color: #1a73e8; }
</style>
</head>
<body>
<div class="container">
  <header>
    <h1>오늘뭐입지</h1>
    <p>날씨 기반 AI 옷 추천 서비스</p>
  </header>

  <!-- Region -->
  <div class="card" id="region-card">
    <h2>지역 설정</h2>
    <div id="region-info" class="msg loading">로딩 중...</div>
    <form class="inline" id="region-form">
      <input type="text" name="name" placeholder="지역명 (예: 서울)" required>
      <input type="text" name="latitude" placeholder="위도" required>
      <input type="text" name="longitude" placeholder="경도" required>
      <button type="submit" class="btn-secondary">변경</button>
    </form>
  </div>

  <!-- Weather -->
  <div class="card" id="weather-card">
    <h2>현재 날씨</h2>
    <div id="weather-content" class="msg loading">날씨 불러오는 중...</div>
  </div>

  <!-- AI Recommendation -->
  <div class="card">
    <h2>AI 코디 추천</h2>
    <form class="inline" id="rec-form">
      <input type="text" name="occasion" placeholder="상황 (예: 출근, 데이트)">
      <input type="text" name="style_preference" placeholder="스타일 (예: 캐주얼)">
      <button type="submit" class="btn-primary">추천 받기</button>
    </form>
    <div id="rec-result"></div>
  </div>

  <!-- Closet -->
  <div class="card">
    <h2>내 옷장</h2>
    <div id="clothes-content" class="msg loading">옷장 불러오는 중...</div>
    <ul class="clothes-list" id="clothes-list"></ul>
    <form class="inline" id="clothes-form">
      <input type="text" name="name" placeholder="이름" required>
      <select name="category" required>
        <option value="">카테고리</option>
        <option value="상의">상의</option>
        <option value="하의">하의</option>
        <option value="아우터">아우터</option>
        <option value="신발">신발</option>
        <option value="가방">가방</option>
        <option value="모자">모자</option>
        <option value="벨트">벨트</option>
        <option value="액세서리">액세서리</option>
      </select>
      <select name="thickness">
        <option value="medium">보통</option>
        <option value="thin">얇은</option>
        <option value="thick">두꺼운</option>
      </select>
      <button type="submit" class="btn-primary">추가</button>
    </form>
    <div style="margin-top: 16px; border-top: 1px solid #eee; padding-top: 12px;">
      <h3 style="font-size: 0.95rem; margin-bottom: 8px;">사진으로 등록</h3>
      <div class="upload-area" id="upload-area">
        <input type="file" id="image-input" accept="image/*">
        <p style="color: #888; font-size: 0.85rem;">클릭하거나 이미지를 드래그하여 업로드</p>
        <img id="upload-preview" class="upload-preview" style="display:none;">
      </div>
      <div style="text-align: center;">
        <button id="upload-btn" class="btn-upload" style="display:none;" disabled>AI로 분석하여 등록</button>
      </div>
      <div id="upload-status"></div>
    </div>
  </div>
</div>

<div class="modal-overlay" id="detail-modal">
  <div class="modal">
    <button class="modal-close" id="modal-close">&times;</button>
    <h3 id="modal-title"></h3>
    <div id="modal-body"></div>
  </div>
</div>

<script>
const API = '/api';

async function fetchJSON(url, opts) {
  const res = await fetch(url, opts);
  if (!res.ok) {
    const body = await res.text();
    throw new Error(body || res.statusText);
  }
  return res.json();
}

// --- Region ---
async function loadRegion() {
  const el = document.getElementById('region-info');
  try {
    const r = await fetchJSON(API + '/region');
    el.className = 'msg';
    el.textContent = `${r.name} (${r.latitude}, ${r.longitude})`;
  } catch {
    el.className = 'msg error';
    el.textContent = '지역이 설정되지 않았습니다. 아래에서 설정해 주세요.';
  }
}
document.getElementById('region-form').addEventListener('submit', async (e) => {
  e.preventDefault();
  const fd = new FormData(e.target);
  const el = document.getElementById('region-info');
  try {
    await fetchJSON(API + '/region', {
      method: 'PUT',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({
        name: fd.get('name'),
        latitude: parseFloat(fd.get('latitude')),
        longitude: parseFloat(fd.get('longitude')),
      }),
    });
    e.target.reset();
    await loadRegion();
    await loadWeather();
  } catch (err) {
    el.className = 'msg error';
    el.textContent = '변경 실패: ' + err.message;
  }
});

// --- Weather ---
async function loadWeather() {
  const el = document.getElementById('weather-content');
  try {
    const w = await fetchJSON(API + '/weather');
    el.className = '';
    el.innerHTML = `
      <div class="weather-grid">
        <div><div class="val">${w.current.temperature}°</div><div class="lbl">기온</div></div>
        <div><div class="val">${w.current.apparent_temperature}°</div><div class="lbl">체감</div></div>
        <div><div class="val">${w.current.weather_description}</div><div class="lbl">날씨</div></div>
      </div>
      <p style="text-align:center;margin-top:8px;font-size:0.8rem;color:#888">
        ${w.region_name} · 습도 ${w.current.humidity}% · 풍속 ${w.current.wind_speed}m/s
      </p>`;
  } catch {
    el.className = 'msg error';
    el.textContent = '날씨를 불러올 수 없습니다. 지역을 먼저 설정해 주세요.';
  }
}

// --- Clothes ---
async function loadClothes() {
  const el = document.getElementById('clothes-content');
  const list = document.getElementById('clothes-list');
  try {
    const items = await fetchJSON(API + '/clothes');
    el.style.display = 'none';
    list.innerHTML = '';
    if (items.length === 0) {
      el.style.display = '';
      el.className = 'msg';
      el.textContent = '등록된 옷이 없습니다.';
      return;
    }
    items.forEach((c) => {
      const li = document.createElement('li');
      const info = document.createElement('span');
      info.className = 'item-info';
      info.innerHTML = `<strong>${c.name}</strong> <span class="tag">${c.category}</span>` +
        (c.color ? ` <span class="tag">${c.color}</span>` : '') +
        ` <span class="tag">${c.thickness}</span>` +
        c.seasons.map(s => ` <span class="tag">${s}</span>`).join('');
      info.onclick = () => showDetail(c);
      const btn = document.createElement('button');
      btn.className = 'btn-danger';
      btn.textContent = '삭제';
      btn.onclick = async () => {
        await fetchJSON(API + '/clothes/' + c.id, { method: 'DELETE' });
        loadClothes();
      };
      li.appendChild(info);
      li.appendChild(btn);
      list.appendChild(li);
    });
  } catch {
    el.style.display = '';
    el.className = 'msg error';
    el.textContent = '옷장을 불러올 수 없습니다.';
  }
}
document.getElementById('clothes-form').addEventListener('submit', async (e) => {
  e.preventDefault();
  const fd = new FormData(e.target);
  try {
    await fetchJSON(API + '/clothes', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({
        name: fd.get('name'),
        category: fd.get('category'),
        thickness: fd.get('thickness'),
      }),
    });
    e.target.reset();
    loadClothes();
  } catch (err) {
    alert('추가 실패: ' + err.message);
  }
});

// --- Recommendation ---
document.getElementById('rec-form').addEventListener('submit', async (e) => {
  e.preventDefault();
  const el = document.getElementById('rec-result');
  const fd = new FormData(e.target);
  el.innerHTML = '<div class="msg loading">AI가 코디를 고민 중...</div>';
  try {
    const body = {};
    if (fd.get('occasion')) body.occasion = fd.get('occasion');
    if (fd.get('style_preference')) body.style_preference = fd.get('style_preference');
    const r = await fetchJSON(API + '/recommendation', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify(body),
    });
    let html = `<div class="rec-result">`;
    html += `<p><strong>${r.weather_summary}</strong></p>`;
    html += `<p style="margin-top:8px">${r.recommendation}</p>`;
    if (r.outfit && r.outfit.length) {
      html += `<div class="outfit-grid">`;
      r.outfit.forEach((o) => {
        html += `<div class="outfit-item">`;
        if (o.image_url && o.image_url.startsWith('data:image/')) {
          html += `<img class="outfit-img" src="${o.image_url}" alt="${o.name}">`;
        }
        html += `<div class="outfit-body"><span class="cat">${o.category}</span> ${o.name}<br><small>${o.reason}</small></div>`;
        html += `</div>`;
      });
      html += `</div>`;
    }
    if (r.tips && r.tips.length) {
      html += `<ul class="tips">`;
      r.tips.forEach((t) => { html += `<li>${t}</li>`; });
      html += `</ul>`;
    }
    html += `</div>`;
    el.innerHTML = html;
  } catch (err) {
    el.innerHTML = `<div class="msg error">추천 실패: ${err.message}</div>`;
  }
});

// --- Detail Modal ---
const detailModal = document.getElementById('detail-modal');
document.getElementById('modal-close').onclick = () => detailModal.classList.remove('open');
detailModal.onclick = (e) => { if (e.target === detailModal) detailModal.classList.remove('open'); };

const thicknessMap = { thin: '얇은', medium: '보통', thick: '두꺼운' };
function showDetail(c) {
  document.getElementById('modal-title').textContent = c.name;
  let html = '';
  if (c.image_url && c.image_url.startsWith('data:image/')) {
    html += '<div style="text-align:center;margin-bottom:12px;"><img src="' + c.image_url + '" style="max-width:100%;max-height:240px;border-radius:8px;"></div>';
  }
  const rows = [
    ['카테고리', c.category],
    ['색상', c.color || '-'],
    ['두께', thicknessMap[c.thickness] || c.thickness],
    ['계절', c.seasons.length ? c.seasons.join(', ') : '-'],
    ['등록일', new Date(c.created_at).toLocaleDateString('ko-KR')],
    ['수정일', new Date(c.updated_at).toLocaleDateString('ko-KR')],
  ];
  html += rows.map(([l, v]) =>
    '<div class="detail-row"><span class="detail-label">' + l + '</span><span class="detail-value">' + v + '</span></div>'
  ).join('');
  document.getElementById('modal-body').innerHTML = html;
  detailModal.classList.add('open');
}

// --- Image Upload ---
const uploadArea = document.getElementById('upload-area');
const imageInput = document.getElementById('image-input');
const uploadPreview = document.getElementById('upload-preview');
const uploadBtn = document.getElementById('upload-btn');
const uploadStatus = document.getElementById('upload-status');
let selectedImageData = null;

uploadArea.addEventListener('click', () => imageInput.click());
uploadArea.addEventListener('dragover', (e) => { e.preventDefault(); uploadArea.classList.add('dragging'); });
uploadArea.addEventListener('dragleave', () => uploadArea.classList.remove('dragging'));
uploadArea.addEventListener('drop', (e) => {
  e.preventDefault();
  uploadArea.classList.remove('dragging');
  const file = e.dataTransfer.files[0];
  if (file && file.type.startsWith('image/')) handleFile(file);
});
imageInput.addEventListener('change', (e) => { if (e.target.files[0]) handleFile(e.target.files[0]); });

function handleFile(file) {
  const reader = new FileReader();
  reader.onload = (e) => {
    selectedImageData = e.target.result;
    uploadPreview.src = selectedImageData;
    uploadPreview.style.display = 'block';
    uploadBtn.style.display = '';
    uploadBtn.disabled = false;
    uploadStatus.innerHTML = '';
  };
  reader.readAsDataURL(file);
}

uploadBtn.addEventListener('click', async () => {
  if (!selectedImageData) return;
  uploadBtn.disabled = true;
  uploadStatus.innerHTML = '<div class="msg loading">AI가 이미지를 분석 중...</div>';
  try {
    const result = await fetchJSON(API + '/clothes/upload', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ image_data: selectedImageData }),
    });
    uploadStatus.innerHTML = '<div class="msg" style="color:#34a853;">' +
      result.name + ' (' + result.category + ') 등록 완료!</div>';
    selectedImageData = null;
    uploadPreview.style.display = 'none';
    uploadBtn.style.display = 'none';
    imageInput.value = '';
    loadClothes();
  } catch (err) {
    let msg = err.message;
    try { const parsed = JSON.parse(msg); if (parsed.error) msg = parsed.error; } catch(e) {}
    uploadStatus.innerHTML = '<div class="msg error">분석 실패: ' + msg + '</div>';
    uploadBtn.disabled = false;
  }
});

// --- Init ---
loadRegion();
loadWeather();
loadClothes();
</script>
</body>
</html>"#;
