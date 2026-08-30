# Lighting 副屏 — Desktop UI

React + TypeScript + Tailwind CSS 实现的桌面客户端界面原型。

## 开发

```bash
cd host-ui
npm install
npm run dev
```

打开终端提示的本地地址（默认 `http://localhost:5173`）。

## 设计 Token

所有视觉数值集中在 `src/styles/tokens.css`，经 `src/index.css` 的 `@theme` 映射到 Tailwind。组件应使用 token / 语义类，避免随意硬编码间距与颜色。

## 结构

- `components/layout` — AppShell / Sidebar / BottomActionBar / StatusBar
- `components/sections` — Hero / ConnectionCard / DisplaySettings / InteractionSettings / PerformancePanel
- `components/ui` — Button / SliderControl / ToggleSwitch / Dropdown / SettingRow / PerformanceCard
