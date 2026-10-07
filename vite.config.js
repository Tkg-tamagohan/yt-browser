import { defineConfig } from "vite";
import { sveltekit } from "@sveltejs/kit/vite";
// @ts-expect-error @types/node 未導入のため process の型が解決しない
import process from "node:process";
const host = process.env.TAURI_DEV_HOST;

// https://vite.dev/config/
export default defineConfig(() => ({
  plugins: [sveltekit()],

  // `tauri dev` / `tauri build` のときだけ適用する Vite オプション
  //
  // 1. Rust のエラーが Vite の画面クリアで見えなくなるのを防ぐ
  clearScreen: false,
  // 2. Tauri は固定ポートを期待するため、使用中なら起動を失敗させる
  server: {
    port: 1420,
    strictPort: true,
    host: host || "127.0.0.1",
    hmr: host
      ? {
          protocol: "ws",
          host,
          port: 1421,
        }
      : undefined,
    watch: {
      // 3. `src-tauri` 配下の変更ではリロードしない
      ignored: ["**/src-tauri/**"],
    },
  },
}));
