// WebView へのファイルドロップによるページ遷移の抑止（仕様決定 BG）。
// `dragDropEnabled: false` で Tauri のドロップハンドラを外すと、ファイルを
// 窓へドロップしたとき WebView がそのファイルへ遷移する。document 全体で
// ファイルのドラッグだけ既定動作を止め、アプリ内の並べ替え（text/plain）は通す。

/// ドラッグ中のデータ型にファイルが含まれるかを判定する
export function isFileDrag(types: ArrayLike<string> | null | undefined): boolean {
  if (!types) return false;
  return Array.from(types).includes("Files");
}

/// document 全体の dragover/drop でファイルのドロップを抑止する。解除関数を返す
export function preventFileDrop(target: Document = document): () => void {
  const handler = (e: DragEvent) => {
    if (isFileDrag(e.dataTransfer?.types)) e.preventDefault();
  };
  target.addEventListener("dragover", handler);
  target.addEventListener("drop", handler);
  return () => {
    target.removeEventListener("dragover", handler);
    target.removeEventListener("drop", handler);
  };
}
