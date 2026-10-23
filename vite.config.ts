import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

// Tauri 的 devUrl 是固定端口，不能随机换端口。
export default defineConfig({
  plugins: [react()],
  // 保留 Rust 侧的日志输出，不要被 Vite 清屏。
  clearScreen: false,
  server: {
    port: 5173,
    strictPort: true,
  },
});
