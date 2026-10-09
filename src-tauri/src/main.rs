// Windows のリリースビルドで余計なコンソールウィンドウを出さないための指定。削除しないこと
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    // Chrome の Native Messaging ホストとして起動されたときは Tauri を組まずに応答して終わる
    // （仕様決定 AH、設計書 §3.4.1）
    if yt_browser_lib::native_host::is_host_invocation(std::env::args()) {
        std::process::exit(yt_browser_lib::native_host::run_host());
    }
    yt_browser_lib::run()
}
