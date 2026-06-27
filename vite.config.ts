import { defineConfig } from "vite";
import vue from "@vitejs/plugin-vue";
import Components from "unplugin-vue-components/vite";
import { NaiveUiResolver } from "unplugin-vue-components/resolvers";

// Tauri 前端构建配置。
// - clearScreen:false 避免 Tauri 清屏覆盖日志
// - server.port 固定 1420（Tauri dev 约定），strictPort 防止换端口
// - envPrefix 限定只暴露 TAURI_ 前缀的环境变量给前端
// - unplugin-vue-components + NaiveUiResolver：Naive UI 组件按需自动注册，
//   模板里写 <n-button> 等无需手动 import / app.use，只打包用到的组件。
const host = process.env.TAURI_DEV_HOST;

export default defineConfig(async () => ({
  plugins: [
    vue(),
    Components({
      resolvers: [NaiveUiResolver()],
      dts: false, // 关闭自动生成 components.d.ts（本仓库手写类型，避免噪音）
    }),
  ],
  clearScreen: false,
  server: {
    port: 1420,
    strictPort: true,
    host: host || false,
    hmr: host
      ? { protocol: "ws", host, port: 1421 }
      : undefined,
    watch: { ignored: ["**/src-tauri/**"] },
  },
  envPrefix: ["VITE_", "TAURI_"],
  build: {
    target: "es2021",
    minify: "esbuild",
    sourcemap: false,
  },
}));
