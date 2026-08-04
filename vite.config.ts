import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

// Tauri 模板标准配置：固定端口 1420，避免自动打开浏览器
export default defineConfig({
  plugins: [react()],
  clearScreen: false,
  server: {
    port: 1420,
    strictPort: true,
    watch: {
      ignored: ["**/src-tauri/**", "**/crates/**", "**/target/**", "**/legacy/**"],
    },
  },
  envPrefix: ["VITE_", "TAURI_"],
  build: {
    target: "es2021",
    minify: "esbuild",
    sourcemap: false,
  },
});
