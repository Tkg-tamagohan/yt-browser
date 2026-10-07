// Tauri には SSR を担う Node.js サーバーがないため、
// adapter-static のフォールバックで SPA モードにする
// 参照: https://svelte.dev/docs/kit/single-page-apps
// 参照: https://v2.tauri.app/start/frontend/sveltekit/
import adapter from "@sveltejs/adapter-static";
import { vitePreprocess } from "@sveltejs/vite-plugin-svelte";

/** @type {import('@sveltejs/kit').Config} */
const config = {
  preprocess: vitePreprocess(),
  kit: {
    adapter: adapter({
      fallback: "index.html",
    }),
  },
};

export default config;
