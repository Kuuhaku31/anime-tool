
// view_watch/src/ui/episode-editor.ts
// 创建按文件哈希共享草稿的剧集 ID 编辑器.
import type { WatchRow } from "../models/watch.ts";
import { WatchStore } from "../state/watch-store.ts";
import { button, element } from "../utils/dom.ts";

/** 渲染剧集 ID 状态和可编辑控件. */
export function episodeEditor(row: WatchRow, store: WatchStore): HTMLElement {
  const container = element("div", undefined, "editor");
  const hash = row.file_sha256;
  if (!hash) {
    container.append(element("span", "无文件, 不可编辑", "muted"));
    return container;
  }
  const state = store.editors.get(hash);
  if (!state) {
    const label = row.ddp_epi_id === null ? "未识别" : row.ddp_epi_id === "-1" ? "无法关联"
      : row.dandanplay_episode_title !== null || row.ddp_ani_id !== null ? `已关联 ${row.ddp_epi_id}`
      : `未解析 ${row.ddp_epi_id}`;
    container.append(element("span", label, "badge"), button("编辑", () => store.edit(hash, row.ddp_epi_id)));
    return container;
  }
  const input = element("input");
  input.type = "text";
  input.inputMode = "numeric";
  input.autocomplete = "off";
  input.placeholder = "空值表示未识别";
  input.value = state.draft;
  input.disabled = state.saving;
  input.dataset.editor = hash;
  input.setAttribute("aria-label", `文件 ${row.file_filename ?? hash} 的剧集 ID`);
  input.addEventListener("input", () => {
    store.draft(hash, input.value);
    for (const other of document.querySelectorAll<HTMLInputElement>("input[data-editor]")) {
      if (other !== input && other.dataset.editor === hash) other.value = input.value;
    }
  });
  input.addEventListener("keydown", (event) => {
    if (event.key === "Enter") { event.preventDefault(); void store.save(hash); }
    if (event.key === "Escape") { event.preventDefault(); store.cancel(hash); }
  });
  const save = button(state.saving ? "保存中" : "保存", () => { void store.save(hash); }, "primary");
  const cancel = button("取消", () => store.cancel(hash));
  save.disabled = state.saving;
  cancel.disabled = state.saving;
  container.append(input, save, cancel);
  if (state.error) {
    const error = element("span", state.error, "error");
    error.setAttribute("role", "alert");
    container.append(error);
  }
  return container;
}
