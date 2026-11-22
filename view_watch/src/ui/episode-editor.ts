
// view_watch/src/ui/episode-editor.ts
// 创建按文件哈希共享草稿的剧集 ID 编辑器.
import type { WatchRow } from "../models/watch.ts";
import { WatchStore } from "../state/watch-store.ts";
import { button, element } from "../utils/dom.ts";

/** 始终显示剧集 ID 输入框, 有文件时允许直接修改并提交. */
export function episodeEditor(row: WatchRow, store: WatchStore): HTMLElement {
  const container = element("div", undefined, "editor");
  const hash = row.file_sha256;
  const state = hash ? store.editors.get(hash) : undefined;
  const input = element("input");
  input.type = "text";
  input.inputMode = "numeric";
  input.autocomplete = "off";
  input.placeholder = !hash ? "无文件, 无法编辑" : state?.saving ? "保存中" : "未识别, 点击输入";
  input.value = state?.draft ?? row.ddp_epi_id ?? "";
  input.disabled = !hash || !!state?.saving;
  input.title = !hash ? "无对应文件, 无法编辑" : state?.saving ? "正在保存" : "编辑剧集 ID";
  if (hash) input.dataset.editor = hash;
  input.setAttribute("aria-label", `文件 ${row.file_filename ?? hash ?? "未知"} 的剧集 ID`);
  container.append(input);
  if (!hash) return container;
  input.addEventListener("input", () => {
    if (store.editors.has(hash)) {
      store.draft(hash, input.value);
    } else {
      store.edit(hash, input.value);
    }
    for (const other of document.querySelectorAll<HTMLInputElement>("input[data-editor]")) {
      if (other !== input && other.dataset.editor === hash && other.value !== input.value) other.value = input.value;
    }
  });
  input.addEventListener("keydown", (event) => {
    if (event.key === "Enter") { event.preventDefault(); void store.save(hash); }
    if (event.key === "Escape") { event.preventDefault(); store.cancel(hash); }
  });
  if (!state) return container;
  const save = button(state.saving ? "保存中" : "保存", () => { void store.save(hash); }, "primary");
  const cancel = button("取消", () => store.cancel(hash));
  save.disabled = state.saving;
  cancel.disabled = state.saving;
  container.append(save, cancel);
  if (state.error) {
    const error = element("span", state.error, "error");
    error.setAttribute("role", "alert");
    container.append(error);
  }
  return container;
}
