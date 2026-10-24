import { defineConfig } from "vitest/config";
import react from "@vitejs/plugin-react";

// 前端测试的配置单独一份，不跟 `vite.config.ts` 混在一起：
// 那份是给 Tauri 的 dev server 用的（固定端口 5173），跟测试没关系。
export default defineConfig({
  plugins: [react()],
  test: {
    // 面板是 DOM，得有个浏览器环境。
    environment: "jsdom",
    setupFiles: ["./src/test/setup.ts"],
  },
});
