// 테스트 전용 호스트 흉내. 실제 ChatGPT·Claude 가 아니다 — 공식 AppBridge 로 iframe 화면과
// postMessage 로 이야기하고, 화면의 tools/call 은 Playwright 가 열어 둔 `window.__callTool`
// (Node 쪽 공식 SDK Client → 진짜 데모 MCP 서버)로 넘긴다.

import { AppBridge, PostMessageTransport } from '@modelcontextprotocol/ext-apps/app-bridge';

window.__modelContext = [];
window.__toolCalls = [];

window.startHost = async ({ html, theme = 'light', toolInput = null }) => {
  const iframe = document.createElement('iframe');
  // 실제 호스트처럼 다른 origin(null) 샌드박스에 둔다.
  iframe.setAttribute('sandbox', 'allow-scripts allow-forms');
  iframe.setAttribute('title', 'MCP App');
  iframe.style.cssText = 'width:100%;border:0;height:300px;display:block;';
  document.getElementById('frame-wrap').append(iframe);

  const bridge = new AppBridge(
    null,
    { name: 'wardrobe-test-host', version: '0.0.0' },
    { serverTools: {}, updateModelContext: { text: {} } },
    { hostContext: { theme, platform: 'web', locale: 'ko-KR', displayMode: 'inline' } },
  );
  bridge.oncalltool = async (params) => {
    window.__toolCalls.push(params);
    return window.__callTool(params);
  };
  bridge.onupdatemodelcontext = async (params) => {
    window.__modelContext.push(params);
    return {};
  };
  bridge.onsizechange = ({ height }) => {
    window.__lastSizeChange = performance.now();
    if (height) iframe.style.height = `${Math.ceil(height)}px`;
  };
  const initialized = new Promise((resolve) => {
    bridge.oninitialized = resolve;
  });
  await bridge.connect(new PostMessageTransport(iframe.contentWindow, iframe.contentWindow));
  iframe.srcdoc = html;
  await initialized;
  window.__bridge = bridge;
  if (toolInput) await bridge.sendToolInput({ arguments: toolInput });
  return true;
};

window.__lastSizeChange = 0;

/** 화면이 보낸 크기 알림이 quietMs 동안 멎을 때까지 기다린다(스크린샷 전에). */
window.settleSize = (quietMs = 250) =>
  new Promise((resolve) => {
    const tick = () => (performance.now() - window.__lastSizeChange >= quietMs ? resolve() : setTimeout(tick, 50));
    setTimeout(tick, quietMs);
  });

window.sendToolResult = (result) => window.__bridge.sendToolResult(result);
