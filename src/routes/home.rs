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
<meta name="viewport" content="width=device-width, initial-scale=1.0, user-scalable=no">
<title>오늘뭐입지</title>
<style>
  :root {
    --primary: #2563eb;
    --primary-light: #dbeafe;
    --primary-dark: #1d4ed8;
    --success: #16a34a;
    --success-light: #dcfce7;
    --warning: #f59e0b;
    --warning-light: #fef3c7;
    --danger: #dc2626;
    --danger-light: #fee2e2;
    --gray-50: #f9fafb;
    --gray-100: #f3f4f6;
    --gray-200: #e5e7eb;
    --gray-300: #d1d5db;
    --gray-400: #9ca3af;
    --gray-500: #6b7280;
    --gray-600: #4b5563;
    --gray-700: #374151;
    --gray-800: #1f2937;
    --gray-900: #111827;
    --radius: 12px;
    --shadow: 0 1px 3px rgba(0,0,0,0.08), 0 1px 2px rgba(0,0,0,0.04);
    --shadow-md: 0 4px 6px -1px rgba(0,0,0,0.1), 0 2px 4px -2px rgba(0,0,0,0.1);
    --shadow-lg: 0 10px 15px -3px rgba(0,0,0,0.1), 0 4px 6px -4px rgba(0,0,0,0.1);
    --tab-height: 60px;
  }
  * { margin: 0; padding: 0; box-sizing: border-box; }
  body {
    font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, "Helvetica Neue", sans-serif;
    background: var(--gray-50);
    color: var(--gray-800);
    max-width: 480px;
    margin: 0 auto;
    padding-bottom: calc(var(--tab-height) + 16px);
    -webkit-font-smoothing: antialiased;
    overflow-x: hidden;
  }

  /* --- Screens --- */
  .screen { display: none; padding: 16px 16px 24px; animation: fadeIn 0.2s ease; }
  .screen.active { display: block; }
  @keyframes fadeIn { from { opacity: 0; transform: translateY(8px); } to { opacity: 1; transform: translateY(0); } }

  /* --- Chat --- */
  #screen-chat.active { display: block; }
  .chat-container {
    padding: 8px 0;
    display: flex; flex-direction: column; gap: 10px;
  }
  .chat-bubble {
    max-width: 85%; padding: 10px 14px; border-radius: 16px;
    font-size: 0.9rem; line-height: 1.5; word-break: break-word;
  }
  .chat-user {
    align-self: flex-end; background: var(--primary); color: #fff;
    border-bottom-right-radius: 4px;
  }
  .chat-ai {
    align-self: flex-start; background: var(--gray-100); color: var(--gray-800);
    border-bottom-left-radius: 4px;
  }
  .chat-ai .chat-item-chip {
    display: inline-block; background: #fff; border: 1px solid var(--gray-200);
    border-radius: 8px; padding: 2px 8px; margin: 2px 2px; font-size: 0.8rem;
  }
  .chat-ai .chat-item-chip.not-owned {
    border-style: dashed; border-color: var(--warning); color: var(--gray-500);
  }

  /* --- Editorial Lookbook Card --- */
  .lookbook-card {
    background: #faf9f7; border-radius: 16px; overflow: hidden;
    max-width: 100%; margin: 8px 0; box-shadow: 0 2px 20px rgba(0,0,0,0.06);
  }
  .lookbook-header {
    padding: 20px 20px 12px; text-align: center;
  }
  .lookbook-title {
    font-size: 1.15rem; font-weight: 700; letter-spacing: 0.5px;
    color: #2d2a26; margin-bottom: 4px; font-family: Georgia, serif;
  }
  .lookbook-subtitle {
    font-size: 0.78rem; color: #8a8580; letter-spacing: 0.3px;
  }
  .lookbook-desc {
    padding: 0 20px 16px; font-size: 0.82rem; line-height: 1.7;
    color: #5a5550; text-align: center; font-style: italic;
  }
  .lookbook-items {
    display: grid; grid-template-columns: repeat(5, 1fr); gap: 6px;
    padding: 0 12px 16px;
  }
  .lookbook-item {
    background: #fff; border-radius: 10px; padding: 10px 6px;
    text-align: center; box-shadow: 0 1px 6px rgba(0,0,0,0.04);
    min-width: 0;
  }
  .lookbook-item.not-owned {
    border: 1.5px dashed #d4a574; background: #fefcfa;
  }
  .lookbook-item-slot {
    font-size: 0.6rem; color: #b0a99f; text-transform: uppercase;
    letter-spacing: 1px; margin-bottom: 4px;
  }
  .lookbook-item-name {
    font-size: 0.72rem; font-weight: 600; color: #3d3a36; line-height: 1.3;
  }
  .lookbook-item-tag {
    font-size: 0.6rem; color: #c4a882; margin-top: 4px;
    font-style: italic;
  }
  .lookbook-mood {
    padding: 0 20px 14px; display: flex; flex-wrap: wrap;
    gap: 6px; justify-content: center;
  }
  .lookbook-mood-chip {
    font-size: 0.65rem; padding: 3px 10px; border-radius: 12px;
    background: #eae6e0; color: #6b6560; letter-spacing: 0.3px;
  }
  .lookbook-actions {
    padding: 0 16px 16px; display: flex; gap: 8px; justify-content: center;
  }
  .lookbook-action-btn {
    font-size: 0.72rem; padding: 6px 14px; border-radius: 16px;
    border: 1px solid #d8d3cc; background: #fff; color: #5a5550;
    cursor: pointer; transition: all 0.2s;
  }
  .lookbook-action-btn:hover { background: #f0ede8; }
  .lookbook-fb {
    padding: 8px 16px 16px; display: flex; gap: 8px; justify-content: center;
  }
  .lookbook-img {
    width: 100%; max-height: 420px; object-fit: cover; border-radius: 8px;
  }
  .chat-input-bar { display: flex; gap: 8px; padding: 12px 0; }
  .chat-input {
    flex: 1; border: 1.5px solid var(--gray-200); border-radius: 20px;
    padding: 10px 16px; font-size: 0.9rem; outline: none;
    transition: border-color 0.2s;
  }
  .chat-input:focus { border-color: var(--primary); }
  .chat-send-btn {
    width: 40px; height: 40px; border-radius: 50%; border: none;
    background: var(--primary); color: #fff; cursor: pointer;
    display: flex; align-items: center; justify-content: center;
    flex-shrink: 0;
  }
  .chat-send-btn:disabled { opacity: 0.5; cursor: default; }
  .chat-typing { color: var(--gray-400); font-style: italic; font-size: 0.85rem; }
  .chat-fb-btn {
    border: 1px solid var(--gray-200); background: #fff; border-radius: 16px;
    padding: 4px 12px; font-size: 0.85rem; cursor: pointer;
  }
  .chat-fb-btn:hover { background: var(--gray-100); }
  .chat-fb-btn.selected { background: var(--primary); color: #fff; border-color: var(--primary); }
  .chat-fb-done {
    font-size: 0.8rem; color: var(--gray-500); padding: 4px 0;
    display: inline-block;
  }

  /* --- Bottom Tab Bar --- */
  .tab-bar {
    position: fixed; bottom: 0; left: 50%; transform: translateX(-50%);
    width: 100%; max-width: 480px;
    height: var(--tab-height);
    background: #fff;
    border-top: 1px solid var(--gray-200);
    display: flex;
    z-index: 100;
    padding-bottom: env(safe-area-inset-bottom, 0);
  }
  .tab-item {
    flex: 1; display: flex; flex-direction: column; align-items: center; justify-content: center;
    gap: 2px; cursor: pointer; color: var(--gray-400); transition: color 0.2s;
    background: none; border: none; font-size: 0.7rem; font-weight: 500;
  }
  .tab-item.active { color: var(--primary); }
  .tab-item svg { width: 24px; height: 24px; }

  /* --- Weather Bar --- */
  .weather-bar {
    background: linear-gradient(135deg, var(--primary), #3b82f6);
    color: #fff;
    padding: 12px 16px;
    border-radius: var(--radius);
    font-size: 0.85rem;
    margin-bottom: 16px;
    display: flex; align-items: center; gap: 8px;
    box-shadow: var(--shadow-md);
  }
  .weather-bar .weather-icon { font-size: 1.2rem; }
  .weather-bar .weather-text { flex: 1; }
  .weather-bar .weather-temp { font-size: 1.1rem; font-weight: 700; }

  /* --- Cards --- */
  .card {
    background: #fff;
    border-radius: var(--radius);
    padding: 16px;
    margin-bottom: 12px;
    box-shadow: var(--shadow);
  }
  .card-title {
    font-size: 0.8rem;
    font-weight: 600;
    color: var(--gray-500);
    text-transform: uppercase;
    letter-spacing: 0.5px;
    margin-bottom: 12px;
  }

  /* --- Buttons --- */
  .btn {
    display: inline-flex; align-items: center; justify-content: center; gap: 6px;
    border: none; border-radius: var(--radius); padding: 12px 20px;
    font-size: 0.9rem; font-weight: 600; cursor: pointer;
    transition: all 0.15s ease;
  }
  .btn-lg { width: 100%; padding: 16px; font-size: 1rem; }
  .btn-primary { background: var(--primary); color: #fff; }
  .btn-primary:hover { background: var(--primary-dark); }
  .btn-primary:active { transform: scale(0.98); }
  .btn-secondary { background: var(--gray-100); color: var(--gray-700); }
  .btn-secondary:hover { background: var(--gray-200); }
  .btn-outline { background: transparent; border: 1.5px solid var(--gray-200); color: var(--gray-700); }
  .btn-outline:hover { border-color: var(--primary); color: var(--primary); }
  .btn-danger { background: var(--danger); color: #fff; font-size: 0.8rem; padding: 8px 14px; }
  .btn-danger:hover { background: #b91c1c; }
  .btn-success { background: var(--success); color: #fff; }
  .btn-success:hover { background: #15803d; }
  .btn-sm { padding: 6px 12px; font-size: 0.8rem; border-radius: 8px; }
  .btn:disabled { opacity: 0.5; cursor: not-allowed; }

  /* --- CTA Buttons on home --- */
  .cta-group { display: flex; flex-direction: column; gap: 10px; margin-bottom: 16px; }
  .cta-primary {
    background: var(--primary); color: #fff;
    border: none; border-radius: var(--radius); padding: 18px 20px;
    font-size: 1.05rem; font-weight: 700; cursor: pointer;
    display: flex; align-items: center; justify-content: center; gap: 8px;
    box-shadow: var(--shadow-md);
    transition: all 0.15s;
  }
  .cta-primary:hover { background: var(--primary-dark); }
  .cta-primary:active { transform: scale(0.98); }
  .cta-secondary {
    background: #fff; color: var(--primary); border: 1.5px solid var(--primary);
    border-radius: var(--radius); padding: 14px 20px;
    font-size: 0.95rem; font-weight: 600; cursor: pointer;
    display: flex; align-items: center; justify-content: center; gap: 8px;
    transition: all 0.15s;
  }
  .cta-secondary:hover { background: var(--primary-light); }

  /* --- Chips / Tags --- */
  .chip {
    display: inline-block; font-size: 0.72rem; font-weight: 500;
    padding: 3px 10px; border-radius: 20px;
    background: var(--primary-light); color: var(--primary);
  }
  .chip-success { background: var(--success-light); color: var(--success); }
  .chip-warning { background: var(--warning-light); color: #92400e; }
  .chip-danger { background: var(--danger-light); color: var(--danger); }
  .chip-neutral { background: var(--gray-100); color: var(--gray-600); }

  /* --- Filter Chips --- */
  .filter-bar { display: flex; gap: 6px; overflow-x: auto; padding-bottom: 8px; margin-bottom: 12px; -webkit-overflow-scrolling: touch; }
  .filter-bar::-webkit-scrollbar { display: none; }
  .filter-chip {
    flex-shrink: 0; padding: 7px 14px; border-radius: 20px;
    font-size: 0.8rem; font-weight: 500; cursor: pointer;
    border: 1.5px solid var(--gray-200); background: #fff; color: var(--gray-600);
    transition: all 0.15s;
  }
  .filter-chip.active { background: var(--primary); color: #fff; border-color: var(--primary); }

  /* --- Summary --- */
  .summary-grid { display: grid; grid-template-columns: repeat(3, 1fr); gap: 8px; }
  .summary-item { text-align: center; padding: 12px 8px; background: var(--gray-50); border-radius: 8px; }
  .summary-val { font-size: 1.3rem; font-weight: 700; color: var(--primary); }
  .summary-lbl { font-size: 0.7rem; color: var(--gray-500); margin-top: 2px; }

  /* --- Wardrobe item cards --- */
  .item-card {
    background: #fff; border-radius: var(--radius); padding: 14px;
    box-shadow: var(--shadow); cursor: pointer;
    transition: box-shadow 0.15s, transform 0.15s;
  }
  .item-card:hover { box-shadow: var(--shadow-md); transform: translateY(-1px); }
  .item-card:active { transform: scale(0.98); }
  .item-card-name { font-weight: 600; font-size: 0.95rem; margin-bottom: 6px; }
  .item-card-tags { display: flex; flex-wrap: wrap; gap: 4px; margin-bottom: 6px; }
  .item-card-seasons { display: flex; gap: 3px; }
  .items-grid { display: grid; grid-template-columns: repeat(2, 1fr); gap: 10px; }

  /* --- Detail screen --- */
  .detail-header { display: flex; align-items: center; gap: 12px; margin-bottom: 16px; }
  .detail-back { background: none; border: none; cursor: pointer; color: var(--gray-500); padding: 4px; }
  .detail-back:hover { color: var(--primary); }
  .detail-title { font-size: 1.2rem; font-weight: 700; flex: 1; }
  .detail-image {
    width: 100%; max-height: 280px; object-fit: cover;
    border-radius: var(--radius); margin-bottom: 16px;
  }
  .detail-section { margin-bottom: 16px; }
  .detail-section-title { font-size: 0.8rem; font-weight: 600; color: var(--gray-500); margin-bottom: 8px; text-transform: uppercase; letter-spacing: 0.5px; }
  .detail-row { display: flex; padding: 8px 0; border-bottom: 1px solid var(--gray-100); font-size: 0.9rem; }
  .detail-row:last-child { border-bottom: none; }
  .detail-label { width: 90px; color: var(--gray-400); flex-shrink: 0; font-size: 0.85rem; }
  .detail-value { flex: 1; font-weight: 500; }
  .role-explain {
    background: var(--primary-light); border-radius: 8px; padding: 12px;
    font-size: 0.85rem; color: var(--primary-dark); line-height: 1.5;
  }

  /* --- Evaluate screen --- */
  .slot-group { margin-bottom: 12px; }
  .slot-label { font-size: 0.8rem; font-weight: 600; color: var(--gray-500); margin-bottom: 6px; }
  .slot-select {
    width: 100%; padding: 10px 12px;
    border: 1.5px solid var(--gray-200); border-radius: 8px;
    font-size: 0.9rem; color: var(--gray-800);
    background: #fff; appearance: none;
    background-image: url("data:image/svg+xml,%3Csvg xmlns='http://www.w3.org/2000/svg' width='12' height='12' viewBox='0 0 12 12'%3E%3Cpath fill='%239ca3af' d='M6 8L1 3h10z'/%3E%3C/svg%3E");
    background-repeat: no-repeat; background-position: right 12px center;
    cursor: pointer; transition: border-color 0.15s;
  }
  .slot-select:focus { outline: none; border-color: var(--primary); }

  /* --- Score Circle --- */
  .score-area { text-align: center; padding: 20px 0; }
  .score-circle {
    display: inline-flex; align-items: center; justify-content: center;
    width: 100px; height: 100px; border-radius: 50%;
    font-size: 2.2rem; font-weight: 800; color: #fff;
    box-shadow: var(--shadow-lg);
  }
  .score-great { background: linear-gradient(135deg, var(--success), #22c55e); }
  .score-good { background: linear-gradient(135deg, var(--primary), #3b82f6); }
  .score-okay { background: linear-gradient(135deg, var(--warning), #fbbf24); }
  .score-awkward { background: linear-gradient(135deg, var(--danger), #ef4444); }
  .verdict-label { font-size: 1.1rem; font-weight: 700; margin-top: 8px; }
  .eval-summary { font-size: 0.9rem; color: var(--gray-500); margin-top: 6px; }

  /* --- Eval Results --- */
  .strength-item, .problem-item {
    display: flex; gap: 8px; padding: 8px 0;
    border-bottom: 1px solid var(--gray-100); font-size: 0.85rem; line-height: 1.4;
  }
  .strength-item:last-child, .problem-item:last-child { border-bottom: none; }
  .strength-icon { color: var(--success); flex-shrink: 0; font-weight: 700; }
  .problem-icon { color: var(--danger); flex-shrink: 0; font-weight: 700; }
  .problem-deduction { font-size: 0.72rem; color: var(--danger); font-weight: 600; }

  .suggestion-card {
    background: var(--gray-50); border-radius: 8px; padding: 12px;
    margin-bottom: 8px; border-left: 3px solid var(--primary);
  }
  .suggestion-type { font-size: 0.72rem; font-weight: 700; color: var(--primary); text-transform: uppercase; margin-bottom: 4px; }
  .suggestion-reason { font-size: 0.85rem; color: var(--gray-700); margin-bottom: 6px; }
  .suggestion-examples { display: flex; flex-wrap: wrap; gap: 4px; }

  .explanation-block {
    background: var(--gray-50); border-radius: 8px; padding: 14px;
    font-size: 0.85rem; line-height: 1.6; color: var(--gray-600);
    white-space: pre-wrap;
  }

  /* --- Rec cards on home --- */
  .rec-card {
    background: #fff; border-radius: var(--radius); padding: 14px;
    box-shadow: var(--shadow); margin-bottom: 10px;
    border-left: 4px solid var(--primary);
  }
  .rec-card-header { display: flex; align-items: center; gap: 10px; margin-bottom: 8px; }
  .rec-score-mini {
    width: 40px; height: 40px; border-radius: 50%;
    display: flex; align-items: center; justify-content: center;
    font-size: 0.9rem; font-weight: 800; color: #fff;
    flex-shrink: 0;
  }
  .rec-verdict { font-weight: 600; font-size: 0.9rem; }
  .rec-items { display: flex; flex-wrap: wrap; gap: 4px; margin-bottom: 6px; }
  .rec-reason { font-size: 0.8rem; color: var(--gray-500); line-height: 1.4; }
  .rec-tip { font-size: 0.78rem; color: var(--warning); margin-top: 6px; display: flex; gap: 4px; }

  /* --- Mode cards --- */
  .mode-card {
    background: #fff; border-radius: var(--radius); padding: 14px;
    box-shadow: var(--shadow); margin-bottom: 10px;
    border-left: 4px solid var(--gray-300);
  }
  .mode-card[data-mode="todays_pick"] { border-left-color: var(--primary); }
  .mode-card[data-mode="variation"] { border-left-color: #7c3aed; }
  .mode-card[data-mode="dormant_revival"] { border-left-color: var(--warning); }
  .mode-header {
    display: flex; align-items: center; justify-content: space-between;
    margin-bottom: 4px;
  }
  .mode-header-left { display: flex; align-items: center; gap: 6px; }
  .mode-icon { font-size: 1rem; }
  .mode-label { font-weight: 700; font-size: 0.88rem; }
  .mode-score {
    font-size: 0.72rem; font-weight: 700; color: #fff;
    padding: 2px 8px; border-radius: 10px; background: var(--gray-400);
  }
  .mode-score.score-great { background: var(--success); }
  .mode-score.score-good { background: var(--primary); }
  .mode-score.score-okay { background: var(--warning); }
  .mode-score.score-awkward { background: var(--danger); }
  .mode-subtitle { font-size: 0.75rem; color: var(--gray-400); margin-bottom: 8px; }
  .mode-reason { font-size: 0.8rem; color: var(--gray-600); line-height: 1.4; margin-top: 6px; }
  .revival-chip {
    background: var(--warning-light); color: #92400e;
    font-size: 0.72rem; padding: 2px 8px; border-radius: 8px;
    display: inline-block; margin-right: 4px; margin-bottom: 4px;
  }
  .mode-detail { display: none; margin-top: 10px; padding-top: 10px; border-top: 1px solid var(--gray-100); }
  .mode-detail.open { display: block; }
  .mode-toggle {
    font-size: 0.75rem; color: var(--primary); cursor: pointer;
    background: none; border: none; padding: 4px 0; margin-top: 4px;
  }
  .scoring-bar {
    font-size: 0.72rem; color: var(--gray-400); margin-top: 6px;
    display: flex; gap: 8px; flex-wrap: wrap;
  }
  .scoring-bar span { white-space: nowrap; }

  /* --- Quick actions --- */
  .quick-actions { display: grid; grid-template-columns: repeat(2, 1fr); gap: 8px; margin-top: 8px; }

  /* --- Forms --- */
  .form-group { margin-bottom: 12px; }
  .form-label { display: block; font-size: 0.8rem; font-weight: 600; color: var(--gray-500); margin-bottom: 6px; }
  .form-input {
    width: 100%; padding: 10px 12px;
    border: 1.5px solid var(--gray-200); border-radius: 8px;
    font-size: 0.9rem; color: var(--gray-800);
    transition: border-color 0.15s;
  }
  .form-input:focus { outline: none; border-color: var(--primary); }
  .form-select {
    width: 100%; padding: 10px 12px;
    border: 1.5px solid var(--gray-200); border-radius: 8px;
    font-size: 0.9rem; color: var(--gray-800); background: #fff;
    appearance: none;
    background-image: url("data:image/svg+xml,%3Csvg xmlns='http://www.w3.org/2000/svg' width='12' height='12' viewBox='0 0 12 12'%3E%3Cpath fill='%239ca3af' d='M6 8L1 3h10z'/%3E%3C/svg%3E");
    background-repeat: no-repeat; background-position: right 12px center;
    cursor: pointer; transition: border-color 0.15s;
  }
  .form-select:focus { outline: none; border-color: var(--primary); }

  /* --- Upload area --- */
  .upload-area {
    border: 2px dashed var(--gray-300); border-radius: var(--radius);
    padding: 24px; text-align: center; cursor: pointer;
    transition: all 0.2s;
  }
  .upload-area:hover { border-color: var(--primary); background: var(--primary-light); }
  .upload-area.dragging { border-color: var(--primary); background: var(--primary-light); }
  .upload-area input[type="file"] { display: none; }
  .upload-preview { max-width: 200px; max-height: 200px; margin: 8px auto; border-radius: 8px; }

  /* --- FAB --- */
  .fab {
    position: fixed; bottom: calc(var(--tab-height) + 20px); right: max(16px, calc((100vw - 480px) / 2 + 16px));
    width: 52px; height: 52px; border-radius: 50%;
    background: var(--primary); color: #fff; border: none;
    box-shadow: var(--shadow-lg); cursor: pointer;
    display: flex; align-items: center; justify-content: center;
    font-size: 1.5rem; z-index: 50;
    transition: transform 0.15s, box-shadow 0.15s;
  }
  .fab:hover { transform: scale(1.05); box-shadow: 0 8px 25px rgba(37, 99, 235, 0.4); }
  .fab:active { transform: scale(0.95); }

  /* --- Loading / Messages --- */
  .msg { font-size: 0.85rem; color: var(--gray-400); padding: 8px 0; }
  .msg-loading { color: var(--primary); }
  .msg-error { color: var(--danger); }
  .spinner { display: inline-block; width: 16px; height: 16px; border: 2px solid var(--gray-200); border-top-color: var(--primary); border-radius: 50%; animation: spin 0.6s linear infinite; }
  @keyframes spin { to { transform: rotate(360deg); } }

  .empty-state { text-align: center; padding: 32px 16px; color: var(--gray-400); }
  .empty-state-icon { font-size: 2.5rem; margin-bottom: 8px; }
  .empty-state-text { font-size: 0.9rem; }

  /* --- Region settings (compact on home) --- */
  .region-bar {
    display: flex; align-items: center; gap: 8px; padding: 8px 12px;
    background: var(--gray-100); border-radius: 8px; margin-bottom: 12px;
    font-size: 0.8rem; color: var(--gray-500); cursor: pointer;
  }
  .region-bar:hover { background: var(--gray-200); }

  /* --- Section divider --- */
  .section-gap { height: 8px; }

  /* --- Add item panel (wardrobe) --- */
  .add-panel {
    background: #fff; border-radius: var(--radius);
    padding: 16px; margin-bottom: 12px;
    box-shadow: var(--shadow);
    display: none;
  }
  .add-panel.open { display: block; animation: fadeIn 0.2s ease; }
  .add-panel-header {
    display: flex; align-items: center; justify-content: space-between;
    margin-bottom: 12px;
  }
  .add-panel-title { font-size: 1rem; font-weight: 700; }
  .add-panel-close { background: none; border: none; cursor: pointer; color: var(--gray-400); font-size: 1.2rem; padding: 4px; }

  .tab-toggle { display: flex; gap: 0; margin-bottom: 16px; background: var(--gray-100); border-radius: 8px; padding: 3px; }
  .tab-toggle-item {
    flex: 1; text-align: center; padding: 8px; border-radius: 6px;
    font-size: 0.8rem; font-weight: 600; cursor: pointer;
    color: var(--gray-500); transition: all 0.15s;
    background: transparent; border: none;
  }
  .tab-toggle-item.active { background: #fff; color: var(--gray-800); box-shadow: var(--shadow); }

  /* --- Scrollbar cleanup --- */
  ::-webkit-scrollbar { width: 0; height: 0; }

  /* --- Header --- */
  .screen-header {
    display: flex; align-items: center; gap: 12px;
    padding: 16px 0 12px; margin-bottom: 4px;
  }
  .screen-header h1 { font-size: 1.3rem; font-weight: 800; flex: 1; }
  .screen-header-sub { font-size: 0.8rem; color: var(--gray-400); }

  /* --- Warning banner --- */
  .warning-banner {
    background: var(--warning-light); border-radius: 8px; padding: 10px 12px;
    font-size: 0.8rem; color: #92400e; display: flex; gap: 6px; align-items: flex-start;
    margin-bottom: 12px;
  }

  /* --- Region modal --- */
  .modal-overlay {
    display: none; position: fixed; top: 0; left: 0; right: 0; bottom: 0;
    background: rgba(0,0,0,0.4); z-index: 200;
    justify-content: center; align-items: flex-end;
  }
  .modal-overlay.open { display: flex; }
  .modal-sheet {
    background: #fff; border-radius: var(--radius) var(--radius) 0 0;
    padding: 24px; width: 100%; max-width: 480px;
    max-height: 80vh; overflow-y: auto;
    animation: slideUp 0.25s ease;
  }
  @keyframes slideUp { from { transform: translateY(100%); } to { transform: translateY(0); } }
  .modal-handle { width: 36px; height: 4px; background: var(--gray-300); border-radius: 2px; margin: 0 auto 16px; }
</style>
</head>
<body>

<!-- ========== SCREEN: HOME ========== -->
<div class="screen active" id="screen-home">
  <div class="screen-header">
    <h1>오늘뭐입지</h1>
  </div>

  <div id="weather-bar" class="weather-bar" style="display:none;">
    <span class="weather-icon" id="weather-icon"></span>
    <span class="weather-text" id="weather-text"></span>
    <span class="weather-temp" id="weather-temp"></span>
  </div>
  <div id="weather-bar-loading" class="msg msg-loading" style="margin-bottom:12px;"><span class="spinner"></span> 날씨 불러오는 중...</div>

  <div id="region-bar" class="region-bar" onclick="openRegionModal()" style="display:none;">
    <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="M12 2C8.13 2 5 5.13 5 9c0 5.25 7 13 7 13s7-7.75 7-13c0-3.87-3.13-7-7-7z"/><circle cx="12" cy="9" r="2.5"/></svg>
    <span id="region-name-bar">지역 설정하기</span>
  </div>

  <div class="cta-group">
    <button class="cta-primary" onclick="navigate('evaluate')">
      <svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5"><path d="M9 12l2 2 4-4"/><circle cx="12" cy="12" r="10"/></svg>
      지금 입은 코디 평가하기
    </button>
    <button class="cta-secondary" onclick="loadAndScrollRec()">
      <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="M12 2l3.09 6.26L22 9.27l-5 4.87 1.18 6.88L12 17.77l-6.18 3.25L7 14.14 2 9.27l6.91-1.01L12 2z"/></svg>
      오늘 추천 보기
    </button>
  </div>

  <!-- Wardrobe Summary -->
  <div class="card" id="wardrobe-summary-card" style="display:none;">
    <div class="card-title">내 옷장 요약</div>
    <div class="summary-grid" id="wardrobe-summary"></div>
    <div id="wardrobe-warning" style="margin-top:8px;"></div>
    <div class="quick-actions">
      <button class="btn btn-outline btn-sm" onclick="navigate('wardrobe'); openAddPanel();">옷 등록</button>
      <button class="btn btn-outline btn-sm" onclick="navigate('wardrobe')">옷장 보기</button>
    </div>
  </div>

  <!-- Recommendation Section -->
  <div id="rec-section">
    <div class="card-title" style="margin-top:8px;">AI 추천</div>
    <div id="rec-content"></div>
  </div>
</div>

<!-- ========== SCREEN: EVALUATE ========== -->
<div class="screen" id="screen-evaluate">
  <div class="screen-header">
    <h1>코디 평가</h1>
    <span class="screen-header-sub">내 옷 조합을 분석해요</span>
  </div>

  <div class="card">
    <div class="slot-group">
      <div class="slot-label">상의</div>
      <select class="slot-select" id="slot-top"><option value="">선택 안함</option></select>
    </div>
    <div class="slot-group">
      <div class="slot-label">하의</div>
      <select class="slot-select" id="slot-bottom"><option value="">선택 안함</option></select>
    </div>
    <div class="slot-group">
      <div class="slot-label">아우터</div>
      <select class="slot-select" id="slot-outer"><option value="">선택 안함</option></select>
    </div>
    <div class="slot-group">
      <div class="slot-label">신발</div>
      <select class="slot-select" id="slot-shoes"><option value="">선택 안함</option></select>
    </div>
    <div class="slot-group">
      <div class="slot-label">가방</div>
      <select class="slot-select" id="slot-bag"><option value="">선택 안함</option></select>
    </div>

    <button class="btn btn-primary btn-lg" id="evaluate-btn" onclick="doEvaluate()">
      <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5"><path d="M9 12l2 2 4-4"/><circle cx="12" cy="12" r="10"/></svg>
      평가하기
    </button>
  </div>

  <div id="eval-result"></div>
</div>

<!-- ========== SCREEN: WARDROBE ========== -->
<div class="screen" id="screen-wardrobe">
  <div class="screen-header">
    <h1>옷장</h1>
    <span class="screen-header-sub" id="wardrobe-count"></span>
  </div>

  <div class="filter-bar" id="category-filter"></div>
  <div class="filter-bar" id="role-filter"></div>

  <div id="wardrobe-content"></div>

  <!-- Add panel (hidden by default) -->
  <div class="add-panel" id="add-panel">
    <div class="add-panel-header">
      <span class="add-panel-title">옷 등록하기</span>
      <button class="add-panel-close" onclick="closeAddPanel()">&times;</button>
    </div>
    <div class="tab-toggle">
      <button class="tab-toggle-item active" onclick="switchAddTab('manual', this)">직접 입력</button>
      <button class="tab-toggle-item" onclick="switchAddTab('image', this)">사진 분석</button>
    </div>

    <!-- Manual entry -->
    <div id="add-tab-manual">
      <form id="clothes-form" onsubmit="addClothesManual(event)">
        <div class="form-group">
          <label class="form-label">이름</label>
          <input class="form-input" type="text" name="name" placeholder="예: 네이비 옥스포드 셔츠" required>
        </div>
        <div style="display:grid; grid-template-columns:1fr 1fr; gap:8px;">
          <div class="form-group">
            <label class="form-label">카테고리</label>
            <select class="form-select" name="category" required>
              <option value="">선택</option>
              <option value="상의">상의</option>
              <option value="하의">하의</option>
              <option value="아우터">아우터</option>
              <option value="신발">신발</option>
              <option value="가방">가방</option>
            </select>
          </div>
          <div class="form-group">
            <label class="form-label">두께</label>
            <select class="form-select" name="thickness">
              <option value="medium">보통</option>
              <option value="thin">얇은</option>
              <option value="thick">두꺼운</option>
            </select>
          </div>
        </div>
        <button type="submit" class="btn btn-primary btn-lg">등록하기</button>
      </form>
    </div>

    <!-- Image upload -->
    <div id="add-tab-image" style="display:none;">
      <div class="upload-area" id="upload-area">
        <input type="file" id="image-input" accept="image/*">
        <svg width="32" height="32" viewBox="0 0 24 24" fill="none" stroke="var(--gray-300)" stroke-width="1.5"><rect x="3" y="3" width="18" height="18" rx="2"/><circle cx="8.5" cy="8.5" r="1.5"/><path d="M21 15l-5-5L5 21"/></svg>
        <p style="color:var(--gray-400); font-size:0.85rem; margin-top:8px;">클릭하거나 이미지를 드래그하여 업로드</p>
        <img id="upload-preview" class="upload-preview" style="display:none;">
      </div>
      <div style="text-align:center; margin-top:12px;">
        <button id="upload-btn" class="btn btn-success btn-lg" style="display:none;" disabled onclick="uploadImage()">AI로 분석하여 등록</button>
      </div>
      <div id="upload-status"></div>
    </div>
  </div>

  <!-- FAB -->
  <button class="fab" id="wardrobe-fab" onclick="toggleAddPanel()" style="display:none;">+</button>
</div>

<!-- ========== SCREEN: DETAIL ========== -->
<div class="screen" id="screen-detail">
  <div class="detail-header">
    <button class="detail-back" onclick="goBackFromDetail()">
      <svg width="24" height="24" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="M15 18l-6-6 6-6"/></svg>
    </button>
    <span class="detail-title" id="detail-title"></span>
    <button class="btn btn-danger btn-sm" id="detail-delete-btn" onclick="deleteFromDetail()">삭제</button>
  </div>
  <div id="detail-content"></div>
</div>

<!-- ========== Region Modal ========== -->
<div class="modal-overlay" id="region-modal">
  <div class="modal-sheet">
    <div class="modal-handle"></div>
    <h3 style="font-size:1.1rem; font-weight:700; margin-bottom:16px;">지역 설정</h3>
    <div id="region-current" class="msg" style="margin-bottom:12px;"></div>
    <form id="region-form" onsubmit="updateRegion(event)">
      <div class="form-group">
        <label class="form-label">지역명</label>
        <input class="form-input" type="text" name="name" placeholder="예: 서울" required>
      </div>
      <div style="display:grid; grid-template-columns:1fr 1fr; gap:8px;">
        <div class="form-group">
          <label class="form-label">위도</label>
          <input class="form-input" type="text" name="latitude" placeholder="37.5665" required>
        </div>
        <div class="form-group">
          <label class="form-label">경도</label>
          <input class="form-input" type="text" name="longitude" placeholder="126.978" required>
        </div>
      </div>
      <div style="display:flex; gap:8px;">
        <button type="button" class="btn btn-secondary" style="flex:1;" onclick="closeRegionModal()">취소</button>
        <button type="submit" class="btn btn-primary" style="flex:1;">저장</button>
      </div>
    </form>
  </div>
</div>

<!-- ========== SCREEN: CHAT ========== -->
<div class="screen" id="screen-chat">
  <div class="screen-header">
    <h1>스타일 상담</h1>
    <span class="screen-header-sub">옷장 기반 코디 질문</span>
  </div>
  <div class="chat-container" id="chat-messages">
    <div class="chat-bubble chat-ai">
      안녕하세요! 옷장에 있는 아이템 기반으로 코디를 추천해드려요.<br>
      <span style="color:var(--gray-400); font-size:0.8rem;">예: "네이비 스니커에 맞는 상하의 추천해줘"</span>
    </div>
  </div>

  <form class="chat-input-bar" onsubmit="sendChat(event)">
    <input type="text" class="chat-input" id="chat-input" placeholder="코디 질문을 입력하세요..." autocomplete="off">
    <button type="submit" class="chat-send-btn" id="chat-send-btn">
      <svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5"><path d="M22 2L11 13"/><path d="M22 2l-7 20-4-9-9-4 20-7z"/></svg>
    </button>
  </form>
</div>

<!-- ========== Bottom Tab Bar ========== -->
<nav class="tab-bar">
  <button class="tab-item active" data-tab="home" onclick="navigate('home')">
    <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="M3 9l9-7 9 7v11a2 2 0 01-2 2H5a2 2 0 01-2-2z"/><polyline points="9 22 9 12 15 12 15 22"/></svg>
    <span>홈</span>
  </button>
  <button class="tab-item" data-tab="chat" onclick="navigate('chat')">
    <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="M21 15a2 2 0 01-2 2H7l-4 4V5a2 2 0 012-2h14a2 2 0 012 2z"/></svg>
    <span>상담</span>
  </button>
  <button class="tab-item" data-tab="evaluate" onclick="navigate('evaluate')">
    <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="M9 12l2 2 4-4"/><circle cx="12" cy="12" r="10"/></svg>
    <span>평가</span>
  </button>
  <button class="tab-item" data-tab="wardrobe" onclick="navigate('wardrobe')">
    <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="M20 7h-4V4a1 1 0 00-1-1H9a1 1 0 00-1 1v3H4a1 1 0 00-1 1v12a1 1 0 001 1h16a1 1 0 001-1V8a1 1 0 00-1-1zM10 5h4v2h-4V5z"/></svg>
    <span>옷장</span>
  </button>
</nav>

<script>
/* ===== GLOBALS ===== */
const API = '/api';
let allClothes = [];
let weatherData = null;
let regionData = null;
let currentDetailId = null;
let activeCategory = '전체';
let activeRole = '전체';
let previousScreen = 'wardrobe';

/* ===== LOOKBOOK HELPERS ===== */
function generateLookbookTitle(items) {
  const styles = items.map(i => i.name).join(' ');
  if (styles.includes('워크') || styles.includes('데님')) return 'Soft Workwear Casual';
  if (styles.includes('밀리터리') || styles.includes('올리브') || styles.includes('유틸리티')) return 'Muted Utility Layers';
  if (styles.includes('니트') || styles.includes('하링턴')) return 'Faded Ivy Mood';
  if (styles.includes('슬랙스') || styles.includes('옥스포드')) return 'Clean Minimal Silhouette';
  if (styles.includes('부츠') || styles.includes('코듀로이')) return 'Grounded Vintage';
  return 'Curated Daily Style';
}

function extractMoodChips(items) {
  const chips = new Set();
  items.forEach(i => {
    const n = i.name;
    if (n.includes('워시드') || n.includes('페이디드')) chips.add('washed');
    if (n.includes('슬러브') || n.includes('멜란지')) chips.add('slubby');
    if (n.includes('스웨이드')) chips.add('suede');
    if (n.includes('데님')) chips.add('denim');
    if (n.includes('캔버스')) chips.add('canvas');
    if (n.includes('린넨') || n.includes('리넨')) chips.add('linen');
    if (n.includes('나일론')) chips.add('nylon');
    if (n.includes('레더')) chips.add('leather');
    if (n.includes('니트')) chips.add('knit');
    if (n.includes('코듀로이')) chips.add('corduroy');
    if (n.includes('울')) chips.add('wool');
    if (n.includes('올리브')) chips.add('muted olive');
    if (n.includes('인디고')) chips.add('indigo');
    if (n.includes('차콜')) chips.add('charcoal');
    if (n.includes('크림') || n.includes('오트밀')) chips.add('cream');
    if (n.includes('네이비')) chips.add('navy');
  });
  return [...chips].slice(0, 6);
}

async function generateOutfitImage(items) {
  try {
    const itemDescs = items.map(i => {
      const slot = {inner:'top/inner',outer:'outerwear',bottom:'pants',shoes:'shoes',bag:'bag'}[i.slot] || i.slot;
      return slot + ': ' + i.name;
    }).join(', ');
    const r = await fetchJSON(API + '/chat/image', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ items: itemDescs }),
    });
    return r.image_url || null;
  } catch (e) { return null; }
}

/* ===== FEEDBACK ===== */
const LIKE_REASONS = ['good_texture_balance','good_grounding','good_color_depth','good_denim_bridge','good_body_balance'];
const DISLIKE_REASONS = ['too_military','too_dark','too_flat','too_light','floating_balance','color_repetition','style_overload'];
const REASON_LABELS = {
  good_texture_balance:'질감 좋음', good_grounding:'안정감 좋음', good_color_depth:'색감 좋음',
  good_denim_bridge:'데님 조합 좋음', good_body_balance:'체형 밸런스 좋음',
  too_military:'너무 군복 같음', too_dark:'너무 어두움', too_flat:'너무 밋밋함',
  too_light:'너무 가벼움', floating_balance:'떠 보임', color_repetition:'색 반복',
  style_overload:'스타일 과다'
};

function showReasons(type, fbId) {
  const reasons = type === 'like' ? LIKE_REASONS : DISLIKE_REASONS;
  const el = document.getElementById(fbId);
  if (!el) return;
  let html = `<div style="display:flex; flex-wrap:wrap; gap:4px; margin-top:4px;">`;
  reasons.forEach(r => {
    html += `<button class="chat-fb-btn" style="font-size:0.75rem;" onclick="submitFeedback('${type}',['${r}'],'${fbId}')">${REASON_LABELS[r]||r}</button>`;
  });
  html += `<button class="chat-fb-btn" style="font-size:0.75rem;" onclick="submitFeedback('${type}',[],'${fbId}')">그냥 ${type==='like'?'👍':'👎'}</button>`;
  html += `</div>`;
  el.innerHTML = html;
}

async function submitFeedback(type, reasons, fbId) {
  const items = window['_fb_' + fbId] || [];
  const body = { feedback_type: type, reasons: reasons };
  items.forEach(it => {
    if (it.slot === 'inner') body.inner_name = it.name;
    else if (it.slot === 'outer') body.outer_name = it.name;
    else if (it.slot === 'bottom') body.bottom_name = it.name;
    else if (it.slot === 'shoes') body.shoes_name = it.name;
    else if (it.slot === 'bag') body.bag_name = it.name;
  });
  try {
    await fetchJSON(API + '/feedback', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify(body),
    });
    const el = document.getElementById(fbId);
    if (el) {
      const label = type === 'like' ? '👍' : '👎';
      const reasonText = reasons.length > 0 ? reasons.map(r=>REASON_LABELS[r]||r).join(', ') : '';
      const msgs = type === 'like'
        ? ['다음에도 이런 조합 위주로 추천할게요', '취향 반영했어요', '비슷한 스타일 더 찾아볼게요']
        : ['다음엔 이런 조합은 줄일게요', '반영했어요, 다른 방향으로 시도해볼게요', '알겠어요, 조정할게요'];
      const msg = msgs[Math.floor(Math.random() * msgs.length)];
      el.innerHTML = `<span class="chat-fb-done">${label} ${msg}${reasonText ? ' — ' + reasonText : ''}</span>`;
      el.style.animation = 'fadeIn 0.3s ease';
    }
  } catch (e) { console.error('feedback error', e); }
}

/* ===== CHAT ===== */
async function sendChat(e) {
  e.preventDefault();
  const input = document.getElementById('chat-input');
  const msg = input.value.trim();
  if (!msg) return;

  const container = document.getElementById('chat-messages');
  const btn = document.getElementById('chat-send-btn');

  // 유저 메시지 추가
  container.innerHTML += `<div class="chat-bubble chat-user">${escHtml(msg)}</div>`;
  input.value = '';
  btn.disabled = true;

  // 타이핑 인디케이터
  const typingId = 'typing-' + Date.now();
  container.innerHTML += `<div class="chat-bubble chat-ai chat-typing" id="${typingId}">생각하는 중...</div>`;
  container.scrollTop = container.scrollHeight;

  try {
    const r = await fetchJSON(API + '/chat', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ message: msg }),
    });
    const el = document.getElementById(typingId);
    if (el) el.remove();

    // AI 응답 렌더링 — editorial lookbook card
    let html = '';
    if (r.items && r.items.length > 0) {
      const fbId = 'fb-' + Date.now();
      window['_fb_' + fbId] = r.items;

      // archetype 제목 생성
      const title = generateLookbookTitle(r.items);
      const desc = r.reply || '';

      html += `<div class="lookbook-card">`;

      // 헤더
      html += `<div class="lookbook-header">`;
      html += `<div class="lookbook-title">${escHtml(title)}</div>`;
      if (weatherData) {
        html += `<div class="lookbook-subtitle">${weatherData.temperature || ''}°C · ${weatherData.weather_description || ''}</div>`;
      }
      html += `</div>`;

      // AI 이미지 (비동기 로딩)
      const imgId = 'img-' + Date.now();
      html += `<div id="${imgId}" style="text-align:center; padding:8px 20px;">`;
      html += `<div style="color:#b0a99f; font-size:0.75rem; padding:30px 0;">이미지 생성 중...</div>`;
      html += `</div>`;

      // 스타일 설명
      if (desc) {
        html += `<div class="lookbook-desc">${escHtml(desc)}</div>`;
      }

      // 이미지 비동기 생성
      setTimeout(async () => {
        const url = await generateOutfitImage(r.items);
        const el = document.getElementById(imgId);
        if (el && url) {
          el.innerHTML = `<img src="${url}" style="width:100%; border-radius:8px; max-height:400px; object-fit:cover;" alt="outfit">`;
        } else if (el) {
          el.innerHTML = '';
        }
      }, 100);

      // 아이템 카드 스크롤
      html += `<div class="lookbook-items">`;
      const slotOrder = ['inner','outer','bottom','shoes','bag'];
      const slotLabel = {inner:'INNER',outer:'OUTER',bottom:'BOTTOM',shoes:'SHOES',bag:'BAG'};
      slotOrder.forEach(slot => {
        const it = r.items.find(i => i.slot === slot);
        if (!it) return;
        const cls = it.owned === false ? 'lookbook-item not-owned' : 'lookbook-item';
        const tag = it.owned === false ? '미보유' : '';
        html += `<div class="${cls}">`;
        html += `<div class="lookbook-item-slot">${slotLabel[slot] || slot}</div>`;
        html += `<div class="lookbook-item-name">${escHtml(it.name)}</div>`;
        if (tag) html += `<div class="lookbook-item-tag">${tag}</div>`;
        html += `</div>`;
      });
      html += `</div>`;

      // 무드 칩
      html += `<div class="lookbook-mood">`;
      const moods = extractMoodChips(r.items);
      moods.forEach(m => {
        html += `<span class="lookbook-mood-chip">${escHtml(m)}</span>`;
      });
      html += `</div>`;

      // 피드백
      html += `<div class="lookbook-fb" id="${fbId}">`;
      html += `<button class="lookbook-action-btn" onclick="showReasons('like','${fbId}')">👍 좋아요</button>`;
      html += `<button class="lookbook-action-btn" onclick="showReasons('dislike','${fbId}')">👎 아쉬워요</button>`;
      html += `</div>`;

      html += `</div>`;
    } else {
      html = `<div class="chat-bubble chat-ai">${r.reply ? r.reply.replace(/\n/g, '<br>') : '추천 결과가 없습니다.'}</div>`;
    }
    container.innerHTML += html;
  } catch (err) {
    const el = document.getElementById(typingId);
    if (el) el.remove();
    container.innerHTML += `<div class="chat-bubble chat-ai" style="color:var(--danger);">오류가 발생했어요: ${escHtml(err.message)}</div>`;
  }

  btn.disabled = false;
  container.scrollTop = container.scrollHeight;
  input.focus();
}

/* ===== API HELPER ===== */
async function fetchJSON(url, opts) {
  const res = await fetch(url, opts);
  if (!res.ok) {
    const body = await res.text();
    throw new Error(body || res.statusText);
  }
  return res.json();
}

/* ===== ROUTING ===== */
function navigate(screen, params) {
  const screens = document.querySelectorAll('.screen');
  screens.forEach(s => s.classList.remove('active'));

  const tabs = document.querySelectorAll('.tab-item');
  tabs.forEach(t => t.classList.remove('active'));

  let tabName = screen;
  if (screen === 'detail') tabName = 'wardrobe';

  const tab = document.querySelector(`.tab-item[data-tab="${tabName}"]`);
  if (tab) tab.classList.add('active');

  const el = document.getElementById('screen-' + screen);
  if (el) { el.classList.add('active'); window.scrollTo(0, 0); }

  if (screen === 'detail' && params && params.id) {
    location.hash = 'detail/' + params.id;
  } else {
    location.hash = screen;
  }

  // Screen-specific logic
  if (screen === 'evaluate') populateSlots();
  if (screen === 'wardrobe') { renderWardrobe(); showFab(true); }
  else { showFab(false); }
  if (screen === 'detail' && params && params.id) showItemDetail(params.id);
}

function showFab(show) {
  const fab = document.getElementById('wardrobe-fab');
  fab.style.display = show ? 'flex' : 'none';
}

function handleHash() {
  const hash = location.hash.replace('#', '') || 'home';
  if (hash.startsWith('detail/')) {
    const id = hash.replace('detail/', '');
    previousScreen = 'wardrobe';
    navigate('detail', { id });
  } else {
    navigate(hash);
  }
}
window.addEventListener('hashchange', handleHash);

function goBackFromDetail() {
  navigate(previousScreen);
}

/* ===== WEATHER ===== */
async function loadWeather() {
  const bar = document.getElementById('weather-bar');
  const loading = document.getElementById('weather-bar-loading');
  try {
    const w = await fetchJSON(API + '/weather');
    weatherData = w;
    loading.style.display = 'none';
    bar.style.display = 'flex';
    const desc = w.current.weather_description || '';
    document.getElementById('weather-icon').textContent = getWeatherEmoji(desc);
    document.getElementById('weather-text').textContent =
      `${w.region_name} · ${desc} · 습도 ${w.current.humidity}%`;
    document.getElementById('weather-temp').textContent = `${w.current.temperature}°C`;
  } catch {
    loading.innerHTML = '<span class="msg msg-error" style="cursor:pointer;" onclick="openRegionModal()">날씨를 불러올 수 없습니다. 지역을 설정해 주세요.</span>';
  }
}

function getWeatherEmoji(desc) {
  if (!desc) return '';
  if (desc.includes('맑')) return '\u2600\uFE0F';
  if (desc.includes('구름') || desc.includes('흐림')) return '\u2601\uFE0F';
  if (desc.includes('비') || desc.includes('소나기')) return '\uD83C\uDF27\uFE0F';
  if (desc.includes('눈')) return '\uD83C\uDF28\uFE0F';
  if (desc.includes('안개')) return '\uD83C\uDF2B\uFE0F';
  return '\uD83C\uDF24\uFE0F';
}

/* ===== REGION ===== */
async function loadRegion() {
  const bar = document.getElementById('region-bar');
  try {
    const r = await fetchJSON(API + '/region');
    regionData = r;
    document.getElementById('region-name-bar').textContent = `${r.name} (${r.latitude}, ${r.longitude})`;
    bar.style.display = 'flex';
  } catch {
    bar.style.display = 'flex';
    document.getElementById('region-name-bar').textContent = '지역 설정하기';
  }
}

function openRegionModal() {
  const modal = document.getElementById('region-modal');
  modal.classList.add('open');
  if (regionData) {
    document.getElementById('region-current').textContent = `현재: ${regionData.name}`;
  } else {
    document.getElementById('region-current').textContent = '지역이 설정되지 않았습니다.';
  }
}

function closeRegionModal() {
  document.getElementById('region-modal').classList.remove('open');
}

document.getElementById('region-modal').addEventListener('click', function(e) {
  if (e.target === this) closeRegionModal();
});

async function updateRegion(e) {
  e.preventDefault();
  const fd = new FormData(e.target);
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
    closeRegionModal();
    await loadRegion();
    await loadWeather();
  } catch (err) {
    alert('변경 실패: ' + err.message);
  }
}

/* ===== CLOTHES DATA ===== */
async function loadClothes() {
  try {
    allClothes = await fetchJSON(API + '/clothes');
  } catch {
    allClothes = [];
  }
  renderWardrobeSummary();
  if (document.getElementById('screen-wardrobe').classList.contains('active')) {
    renderWardrobe();
  }
  populateSlots();
}

/* ===== WARDROBE SUMMARY (HOME) ===== */
function renderWardrobeSummary() {
  const card = document.getElementById('wardrobe-summary-card');
  const grid = document.getElementById('wardrobe-summary');
  const warn = document.getElementById('wardrobe-warning');

  if (allClothes.length === 0) {
    card.style.display = 'none';
    return;
  }
  card.style.display = '';

  const roles = {};
  allClothes.forEach(c => {
    const r = c.role || '미분류';
    roles[r] = (roles[r] || 0) + 1;
  });

  const bab = roles['밥'] || 0;
  const banchan = (roles['반찬'] || 0) + (roles['약한반찬'] || 0);
  const gujo = roles['구조템'] || 0;

  grid.innerHTML = `
    <div class="summary-item"><div class="summary-val">${allClothes.length}</div><div class="summary-lbl">전체</div></div>
    <div class="summary-item"><div class="summary-val">${bab}</div><div class="summary-lbl">밥</div></div>
    <div class="summary-item"><div class="summary-val">${banchan}</div><div class="summary-lbl">반찬</div></div>
  `;

  // Check imbalance
  warn.innerHTML = '';
  if (allClothes.length >= 5) {
    const warnings = [];
    if (bab === 0) warnings.push('밥 역할 아이템이 없어요. 무채색 기본 아이템을 추가해 보세요.');
    if (banchan === 0) warnings.push('반찬 역할 아이템이 없어요. 포인트 아이템을 추가해 보세요.');
    if (gujo === 0) warnings.push('구조템이 없어요. 전체 실루엣을 잡아주는 아이템을 추가해 보세요.');
    if (bab > 0 && banchan > 0 && banchan > bab * 2) warnings.push('반찬이 밥보다 많아요. 기본 아이템을 더 추가하면 좋겠어요.');

    if (warnings.length > 0) {
      warn.innerHTML = warnings.map(w =>
        `<div class="warning-banner"><span>\u26A0\uFE0F</span><span>${w}</span></div>`
      ).join('');
    }
  }
}

/* ===== RECOMMENDATION (HOME) — 3-Mode ===== */
const MODE_STYLES = {
  todays_pick:     { icon: '\u2B50', color: 'var(--primary)', scoreClass: 'score-good' },
  variation:       { icon: '\uD83D\uDD04', color: '#7c3aed', scoreClass: 'score-good' },
  dormant_revival: { icon: '\uD83D\uDC40', color: 'var(--warning)', scoreClass: 'score-okay' },
};

function verdictScoreClass(verdict) {
  if (verdict === '\uD6CC\uB96D\uD574\uC694') return 'score-great';
  if (verdict === '\uC88B\uC544\uC694') return 'score-good';
  if (verdict === '\uAD1C\uCC2E\uC544\uC694') return 'score-okay';
  return 'score-awkward';
}

async function loadRecommendation() {
  const el = document.getElementById('rec-content');
  el.innerHTML = '<div class="msg msg-loading"><span class="spinner"></span> AI\uAC00 \uCF54\uB514\uB97C \uACE0\uBBFC \uC911...</div>';
  try {
    const r = await fetchJSON(API + '/recommendation/multi', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ occasion: '\uC77C\uC0C1' }),
    });
    renderMultiModeResult(r);
  } catch (err) {
    el.innerHTML = `<div class="msg msg-error">\uCD94\uCC9C\uC744 \uBD88\uB7EC\uC62C \uC218 \uC5C6\uC2B5\uB2C8\uB2E4: ${escHtml(err.message)}</div>`;
  }
}

function renderMultiModeResult(r) {
  const el = document.getElementById('rec-content');
  if (!r.modes || r.modes.length === 0) {
    el.innerHTML = '<div class="msg msg-error">\uCD94\uCC9C \uACB0\uACFC\uAC00 \uC5C6\uC2B5\uB2C8\uB2E4</div>';
    return;
  }

  let html = '';

  // Weather summary
  html += `<div style="font-size:0.82rem;color:var(--gray-500);margin-bottom:10px;">${escHtml(r.weather_summary)}</div>`;

  r.modes.forEach((m, idx) => {
    const st = MODE_STYLES[m.mode] || MODE_STYLES.todays_pick;
    const sc = verdictScoreClass(m.verdict);

    html += `<div class="mode-card" data-mode="${escHtml(m.mode)}">`;

    // Header: icon + label
    html += `<div class="mode-header">`;
    html += `<div class="mode-header-left">`;
    html += `<span class="mode-icon">${st.icon}</span>`;
    html += `<span class="mode-label">${escHtml(m.mode_label)}</span>`;
    html += `</div>`;
    html += `</div>`;

    // Subtitle
    html += `<div class="mode-subtitle">${escHtml(m.mode_description)}</div>`;

    // Revival chips (dormant mode)
    if (m.revival_items && m.revival_items.length > 0) {
      m.revival_items.forEach(name => {
        html += `<span class="revival-chip">${escHtml(name)} \u2190 \uC624\uB79C\uB9CC!</span>`;
      });
    }

    // Outfit chips
    if (m.outfit && m.outfit.length) {
      html += `<div class="rec-items">`;
      m.outfit.forEach(o => {
        html += `<span class="chip">${escHtml(o.category)}: ${escHtml(o.name)}</span>`;
      });
      html += `</div>`;
    }

    // Reason
    html += `<div class="mode-reason">${escHtml(m.reason)}</div>`;

    // Toggle button
    html += `<button class="mode-toggle" onclick="toggleModeDetail(${idx})">\uC790\uC138\uD788 \u25BE</button>`;

    // Detail panel (hidden)
    html += `<div class="mode-detail" id="mode-detail-${idx}">`;

    // Items with images
    if (m.outfit && m.outfit.length) {
      m.outfit.forEach(o => {
        html += `<div style="display:flex;gap:10px;align-items:center;margin-bottom:8px;">`;
        if (o.image_url && o.image_url.startsWith('data:image/')) {
          html += `<img src="${o.image_url}" style="width:40px;height:40px;border-radius:6px;object-fit:cover;">`;
        }
        html += `<div>`;
        html += `<div style="font-weight:600;font-size:0.85rem;">${escHtml(o.name)}</div>`;
        html += `<div style="font-size:0.75rem;color:var(--gray-500);">${escHtml(o.category)} \u00B7 ${escHtml(o.reason)}</div>`;
        html += `</div></div>`;
      });
    }

    // Recommendation text
    html += `<div style="font-size:0.8rem;color:var(--gray-600);margin-top:8px;">${escHtml(m.recommendation)}</div>`;

    // Scoring bar
    const sd = m.scoring_detail;
    if (sd) {
      html += `<div class="scoring-bar">`;
      html += `<span>\uC2A4\uD0C0\uC77C ${sd.style_score}</span>`;
      if (sd.recency_penalty > 0) html += `<span>\uBC18\uBCF5 -${sd.recency_penalty}</span>`;
      if (sd.diversity_bonus > 0) html += `<span>\uB2E4\uC591\uC131 +${sd.diversity_bonus}</span>`;
      if (sd.dormant_bonus > 0) html += `<span>\uBD80\uD65C +${sd.dormant_bonus}</span>`;
      html += `<span>\u2192 ${sd.final_score}</span>`;
      html += `</div>`;
    }

    // Tip
    if (m.tips && m.tips.length > 0) {
      html += `<div class="rec-tip"><span>\uD83D\uDCA1</span><span>${escHtml(m.tips[0])}</span></div>`;
    }

    html += `</div>`; // mode-detail
    html += `</div>`; // mode-card
  });

  el.innerHTML = html;
}

function toggleModeDetail(idx) {
  const detail = document.getElementById('mode-detail-' + idx);
  if (detail) {
    detail.classList.toggle('open');
    const btn = detail.previousElementSibling;
    if (btn) btn.textContent = detail.classList.contains('open') ? '\uC811\uAE30 \u25B4' : '\uC790\uC138\uD788 \u25BE';
  }
}

function loadAndScrollRec() {
  const section = document.getElementById('rec-section');
  section.scrollIntoView({ behavior: 'smooth' });
}

/* ===== EVALUATE ===== */
function populateSlots() {
  const categories = {
    'slot-top': '상의',
    'slot-bottom': '하의',
    'slot-outer': '아우터',
    'slot-shoes': '신발',
    'slot-bag': '가방',
  };

  for (const [slotId, cat] of Object.entries(categories)) {
    const sel = document.getElementById(slotId);
    if (!sel) continue;
    const current = sel.value;
    sel.innerHTML = '<option value="">선택 안함</option>';
    allClothes.filter(c => c.category === cat).forEach(c => {
      const opt = document.createElement('option');
      opt.value = c.id;
      opt.textContent = c.name;
      if (current === String(c.id)) opt.selected = true;
      sel.appendChild(opt);
    });
  }
}

function preSelectSlot(slotId, itemId) {
  const sel = document.getElementById(slotId);
  if (sel) sel.value = String(itemId);
}

async function doEvaluate() {
  const btn = document.getElementById('evaluate-btn');
  const resultEl = document.getElementById('eval-result');

  const body = {};
  const top = document.getElementById('slot-top').value;
  const bottom = document.getElementById('slot-bottom').value;
  const outer = document.getElementById('slot-outer').value;
  const shoes = document.getElementById('slot-shoes').value;
  const bag = document.getElementById('slot-bag').value;
  if (top) body.top = top;
  if (bottom) body.bottom = bottom;
  if (outer) body.outer = outer;
  if (shoes) body.shoes = shoes;
  if (bag) body.bag = bag;

  const filledCount = [top, bottom, outer, shoes, bag].filter(Boolean).length;
  if (filledCount < 2) {
    resultEl.innerHTML = '<div class="card"><div class="msg msg-error">최소 2개 이상의 아이템을 선택해주세요.</div></div>';
    return;
  }

  btn.disabled = true;
  btn.innerHTML = '<span class="spinner"></span> 평가 중...';
  resultEl.innerHTML = '';

  try {
    const r = await fetchJSON(API + '/outfit/evaluate', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify(body),
    });
    renderEvalResult(r);
  } catch (err) {
    let msg = err.message;
    try { const p = JSON.parse(msg); if (p.error) msg = p.error; } catch(e) {}
    resultEl.innerHTML = `<div class="card"><div class="msg msg-error">평가 실패: ${escHtml(msg)}</div></div>`;
  } finally {
    btn.disabled = false;
    btn.innerHTML = '<svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5"><path d="M9 12l2 2 4-4"/><circle cx="12" cy="12" r="10"/></svg> 평가하기';
  }
}

function renderEvalResult(r) {
  const el = document.getElementById('eval-result');
  const scoreClass = r.score >= 88 ? 'score-great' : r.score >= 73 ? 'score-good' : r.score >= 55 ? 'score-okay' : 'score-awkward';
  const verdictColor = r.score >= 88 ? 'var(--success)' : r.score >= 73 ? 'var(--primary)' : r.score >= 55 ? 'var(--warning)' : 'var(--danger)';

  let html = '';

  // Score circle
  html += `<div class="card">`;
  html += `<div class="score-area">`;
  html += `<div class="score-circle ${scoreClass}">${r.score}</div>`;
  html += `<div class="verdict-label" style="color:${verdictColor};">${escHtml(r.verdict_label)}</div>`;
  html += `<div class="eval-summary">${escHtml(r.summary)}</div>`;
  html += `</div></div>`;

  // Strengths
  if (r.strengths && r.strengths.length > 0) {
    html += `<div class="card">`;
    html += `<div class="card-title" style="color:var(--success);">강점</div>`;
    r.strengths.forEach(s => {
      html += `<div class="strength-item">`;
      html += `<span class="strength-icon">\u2714</span>`;
      html += `<div><strong>${escHtml(s.rule)}</strong><br><span style="color:var(--gray-500);">${escHtml(s.detail)}</span></div>`;
      html += `</div>`;
    });
    html += `</div>`;
  }

  // Problems
  if (r.problems && r.problems.length > 0) {
    html += `<div class="card">`;
    html += `<div class="card-title" style="color:var(--danger);">문제점</div>`;
    r.problems.forEach(p => {
      html += `<div class="problem-item">`;
      html += `<span class="problem-icon">\u26A0</span>`;
      html += `<div style="flex:1;"><strong>${escHtml(p.rule)}</strong><br><span style="color:var(--gray-500);">${escHtml(p.detail)}</span></div>`;
      html += `<span class="problem-deduction">-${p.deduction}</span>`;
      html += `</div>`;
    });
    html += `</div>`;
  }

  // Suggestions
  if (r.suggestions && r.suggestions.length > 0) {
    html += `<div class="card">`;
    html += `<div class="card-title">제안</div>`;
    r.suggestions.forEach(s => {
      html += `<div class="suggestion-card">`;
      html += `<div class="suggestion-type">${escHtml(s.type)}</div>`;
      html += `<div class="suggestion-reason">${escHtml(s.reason)}</div>`;
      if (s.recommended_examples && s.recommended_examples.length > 0) {
        html += `<div class="suggestion-examples">`;
        s.recommended_examples.forEach(ex => {
          html += `<span class="chip chip-neutral">${escHtml(ex)}</span>`;
        });
        html += `</div>`;
      }
      html += `</div>`;
    });
    html += `</div>`;
  }

  // LLM Explanation
  if (r.explanation) {
    html += `<div class="card">`;
    html += `<div class="card-title">AI 해설</div>`;
    html += `<div class="explanation-block">${escHtml(r.explanation)}</div>`;
    html += `</div>`;
  }

  el.innerHTML = html;
  el.scrollIntoView({ behavior: 'smooth', block: 'start' });
}

/* ===== WARDROBE ===== */
const CATEGORIES = ['전체', '상의', '하의', '아우터', '신발', '가방'];
const ROLES = ['전체', '밥', '반찬', '약한반찬', '연결템', '구조템'];

function initFilters() {
  const catBar = document.getElementById('category-filter');
  catBar.innerHTML = CATEGORIES.map(c =>
    `<button class="filter-chip ${c === activeCategory ? 'active' : ''}" onclick="setCategory('${c}', this)">${c}</button>`
  ).join('');

  const roleBar = document.getElementById('role-filter');
  roleBar.innerHTML = ROLES.map(r =>
    `<button class="filter-chip ${r === activeRole ? 'active' : ''}" onclick="setRole('${r}', this)">${r}</button>`
  ).join('');
}

function setCategory(cat) {
  activeCategory = cat;
  initFilters();
  renderWardrobe();
}

function setRole(role) {
  activeRole = role;
  initFilters();
  renderWardrobe();
}

function renderWardrobe() {
  const el = document.getElementById('wardrobe-content');
  const countEl = document.getElementById('wardrobe-count');

  let items = [...allClothes];

  if (activeCategory !== '전체') {
    items = items.filter(c => c.category === activeCategory);
  }
  if (activeRole !== '전체') {
    items = items.filter(c => c.role === activeRole);
  }

  countEl.textContent = `${items.length}벌`;

  if (items.length === 0) {
    el.innerHTML = `<div class="empty-state">
      <div class="empty-state-icon">\uD83D\uDC55</div>
      <div class="empty-state-text">${allClothes.length === 0 ? '옷장이 비어있어요. 옷을 등록해보세요!' : '해당 조건의 아이템이 없습니다.'}</div>
    </div>`;
    return;
  }

  const thicknessMap = { thin: '얇은', medium: '보통', thick: '두꺼운' };

  let html = '<div class="items-grid">';
  items.forEach(c => {
    html += `<div class="item-card" onclick="navigate('detail', {id:'${c.id}'})">`;
    if (c.image_url && c.image_url.startsWith('data:image/')) {
      html += `<img src="${c.image_url}" style="width:100%;height:80px;object-fit:cover;border-radius:8px;margin-bottom:8px;">`;
    }
    html += `<div class="item-card-name">${escHtml(c.name)}</div>`;
    html += `<div class="item-card-tags">`;
    if (c.role) html += `<span class="chip ${getRoleChipClass(c.role)}">${escHtml(c.role)}</span>`;
    if (c.tone) html += `<span class="chip chip-neutral">${escHtml(c.tone)}</span>`;
    if (c.style) html += `<span class="chip chip-neutral">${escHtml(c.style)}</span>`;
    html += `</div>`;
    if (c.seasons && c.seasons.length > 0) {
      html += `<div class="item-card-seasons">`;
      c.seasons.forEach(s => { html += `<span class="chip chip-neutral" style="font-size:0.65rem;">${s}</span>`; });
      html += `</div>`;
    }
    html += `</div>`;
  });
  html += '</div>';
  el.innerHTML = html;
}

function getRoleChipClass(role) {
  switch(role) {
    case '밥': return '';
    case '반찬': return 'chip-warning';
    case '약한반찬': return 'chip-neutral';
    case '구조템': return 'chip-success';
    case '연결템': return 'chip-neutral';
    default: return 'chip-neutral';
  }
}

/* ===== ADD PANEL (WARDROBE) ===== */
function toggleAddPanel() {
  const panel = document.getElementById('add-panel');
  panel.classList.toggle('open');
}
function openAddPanel() {
  document.getElementById('add-panel').classList.add('open');
}
function closeAddPanel() {
  document.getElementById('add-panel').classList.remove('open');
}

function switchAddTab(tab, btn) {
  document.querySelectorAll('.tab-toggle-item').forEach(t => t.classList.remove('active'));
  btn.classList.add('active');
  document.getElementById('add-tab-manual').style.display = tab === 'manual' ? '' : 'none';
  document.getElementById('add-tab-image').style.display = tab === 'image' ? '' : 'none';
}

async function addClothesManual(e) {
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
    closeAddPanel();
    await loadClothes();
  } catch (err) {
    alert('추가 실패: ' + err.message);
  }
}

/* ===== IMAGE UPLOAD ===== */
let selectedImageData = null;

const uploadArea = document.getElementById('upload-area');
const imageInput = document.getElementById('image-input');
const uploadPreview = document.getElementById('upload-preview');
const uploadBtn = document.getElementById('upload-btn');
const uploadStatus = document.getElementById('upload-status');

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

async function uploadImage() {
  if (!selectedImageData) return;
  uploadBtn.disabled = true;
  uploadBtn.innerHTML = '<span class="spinner"></span> AI가 분석 중...';
  uploadStatus.innerHTML = '';
  try {
    const result = await fetchJSON(API + '/clothes/upload', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ image_data: selectedImageData }),
    });
    uploadStatus.innerHTML = `<div class="msg" style="color:var(--success);">${escHtml(result.name)} (${escHtml(result.category)}) 등록 완료!</div>`;
    selectedImageData = null;
    uploadPreview.style.display = 'none';
    uploadBtn.style.display = 'none';
    imageInput.value = '';
    closeAddPanel();
    await loadClothes();
  } catch (err) {
    let msg = err.message;
    try { const p = JSON.parse(msg); if (p.error) msg = p.error; } catch(e) {}
    uploadStatus.innerHTML = `<div class="msg msg-error">분석 실패: ${escHtml(msg)}</div>`;
    uploadBtn.disabled = false;
    uploadBtn.innerHTML = 'AI로 분석하여 등록';
  }
}

/* ===== ITEM DETAIL ===== */
function showItemDetail(id) {
  const item = allClothes.find(c => String(c.id) === String(id));
  if (!item) {
    document.getElementById('detail-content').innerHTML = '<div class="msg msg-error">아이템을 찾을 수 없습니다.</div>';
    return;
  }
  currentDetailId = item.id;

  document.getElementById('detail-title').textContent = item.name;
  const thicknessMap = { thin: '얇은', medium: '보통', thick: '두꺼운' };

  let html = '';

  // Image
  if (item.image_url && item.image_url.startsWith('data:image/')) {
    html += `<img class="detail-image" src="${item.image_url}" alt="${escHtml(item.name)}">`;
  }

  // Basic info
  html += `<div class="card">`;
  html += `<div class="card-title">기본 정보</div>`;
  const basicRows = [
    ['카테고리', item.category],
    ['색상', item.color || '-'],
    ['두께', thicknessMap[item.thickness] || item.thickness],
    ['계절', item.seasons && item.seasons.length ? item.seasons.join(', ') : '-'],
  ];
  basicRows.forEach(([l, v]) => {
    html += `<div class="detail-row"><span class="detail-label">${l}</span><span class="detail-value">${escHtml(v)}</span></div>`;
  });
  html += `</div>`;

  // Style interpretation
  html += `<div class="card">`;
  html += `<div class="card-title">스타일 분석</div>`;

  // Role explanation
  if (item.role) {
    const roleExplain = getRoleExplanation(item.role);
    html += `<div class="role-explain" style="margin-bottom:12px;">${escHtml(roleExplain)}</div>`;
  }

  // Tags
  const tagPairs = [
    ['역할', item.role],
    ['톤', item.tone],
    ['채도', item.saturation],
    ['색온도', item.color_temperature],
    ['활용도', item.versatility != null ? `${item.versatility}/5` : null],
    ['존재감', item.statement_level != null ? `${item.statement_level}/5` : null],
    ['격식', item.formality_level != null ? `${item.formality_level}/5` : null],
    ['스타일', item.style],
    ['무게감', item.weight],
  ];
  tagPairs.forEach(([label, val]) => {
    if (val) {
      html += `<div class="detail-row"><span class="detail-label">${label}</span><span class="detail-value">${escHtml(String(val))}</span></div>`;
    }
  });

  // Texture worlds
  if (item.texture_worlds && item.texture_worlds.length > 0) {
    html += `<div class="detail-row"><span class="detail-label">텍스처</span><span class="detail-value">`;
    item.texture_worlds.forEach(tw => { html += `<span class="chip chip-neutral" style="margin-right:4px;">${escHtml(tw)}</span>`; });
    html += `</span></div>`;
  }

  html += `</div>`;

  // Action buttons
  html += `<button class="btn btn-primary btn-lg" style="margin-bottom:12px;" onclick="goToEvalWithItem('${item.id}', '${escAttr(item.category)}')">`;
  html += `<svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5"><path d="M9 12l2 2 4-4"/><circle cx="12" cy="12" r="10"/></svg>`;
  html += `이 옷으로 코디 평가`;
  html += `</button>`;

  document.getElementById('detail-content').innerHTML = html;
}

function getRoleExplanation(role) {
  const explanations = {
    '밥': "이 아이템은 '밥' 역할입니다 \u2014 대부분 코디의 중심을 잡아주는 기본 아이템이에요.",
    '반찬': "이 아이템은 '반찬' 역할입니다 \u2014 코디에 포인트와 개성을 더해주는 아이템이에요.",
    '약한반찬': "이 아이템은 '약한반찬' 역할입니다 \u2014 부드럽게 포인트를 더하는 보조 아이템이에요.",
    '구조템': "이 아이템은 '구조템' 역할입니다 \u2014 전체 실루엣과 핏을 잡아주는 아이템이에요.",
    '연결템': "이 아이템은 '연결템' 역할입니다 \u2014 다른 아이템들 사이를 자연스럽게 연결해주는 아이템이에요.",
  };
  return explanations[role] || `이 아이템의 역할: ${role}`;
}

function goToEvalWithItem(id, category) {
  previousScreen = 'detail';
  navigate('evaluate');
  const slotMap = { '상의': 'slot-top', '하의': 'slot-bottom', '아우터': 'slot-outer', '신발': 'slot-shoes', '가방': 'slot-bag' };
  const slotId = slotMap[category];
  if (slotId) {
    setTimeout(() => preSelectSlot(slotId, id), 50);
  }
}

async function deleteFromDetail() {
  if (!currentDetailId) return;
  if (!confirm('정말 삭제하시겠습니까?')) return;
  try {
    await fetchJSON(API + '/clothes/' + currentDetailId, { method: 'DELETE' });
    await loadClothes();
    navigate('wardrobe');
  } catch (err) {
    alert('삭제 실패: ' + err.message);
  }
}

/* ===== UTILS ===== */
function escHtml(str) {
  if (!str) return '';
  const div = document.createElement('div');
  div.textContent = String(str);
  return div.innerHTML;
}

function escAttr(str) {
  return String(str || '').replace(/'/g, "\\'").replace(/"/g, '&quot;');
}

/* ===== INIT ===== */
async function init() {
  initFilters();

  // Load data in parallel
  const promises = [loadRegion(), loadWeather(), loadClothes()];
  await Promise.allSettled(promises);

  // 추천은 버튼 클릭 시에만 실행
  if (allClothes.length > 0) {
    document.getElementById('rec-content').innerHTML =
      '<div class="msg" style="text-align:center; color:var(--gray-400);">위 버튼을 눌러 오늘의 추천을 받아보세요.</div>';
  } else {
    document.getElementById('rec-content').innerHTML =
      '<div class="msg">옷을 등록하면 AI 추천을 받을 수 있어요.</div>';
  }

  // Handle initial hash
  const hash = location.hash.replace('#', '');
  if (hash && hash !== 'home') {
    handleHash();
  }
}

init();
</script>
</body>
</html>"#;
