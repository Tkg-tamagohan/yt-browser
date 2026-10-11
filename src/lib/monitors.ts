// monitor_list コマンド（FR-31、仕様決定 AZ）の戻り値要素。
// 接続中モニター 1 台分。起動ディスプレイ設定の選択肢として使う。
export interface MonitorInfo {
  // モニター名（window.screen に保存する識別子）。取得不能な環境は null。
  name: string | null;
  // 列挙順のインデックス。mpv --screen の番号と同じ並び。
  index: number;
  // 仮想デスクトップ上の左上座標と寸法（物理ピクセル）。表示補助用。
  x: number;
  y: number;
  width: number;
  height: number;
  // プライマリモニターかどうか。表示補助用。
  primary: boolean;
}
