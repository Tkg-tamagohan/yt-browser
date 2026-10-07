// Tauri には SSR を担う Node.js サーバーがないため、
// adapter-static のフォールバックで SPA モードにする
// 参照: https://svelte.dev/docs/kit/single-page-apps
// 参照: https://v2.tauri.app/start/frontend/sveltekit/
export const ssr = false;
