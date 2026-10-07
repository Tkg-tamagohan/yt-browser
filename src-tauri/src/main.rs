// Windows のリリースビルドで余計なコンソールウィンドウを出さないための指定。削除しないこと
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    yt_browser_lib::run()
}
