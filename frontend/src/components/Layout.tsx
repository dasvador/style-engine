import { Outlet } from 'react-router-dom';
import { TabBar } from './TabBar';

/** 화면 공통 껍데기. 기존 `.screen` 클래스를 그대로 쓴다. */
export function Layout() {
  return (
    <>
      <div className="screen active">
        <Outlet />
      </div>
      <TabBar />
    </>
  );
}
