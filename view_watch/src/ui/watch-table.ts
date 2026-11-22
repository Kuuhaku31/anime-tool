
// view_watch/src/ui/watch-table.ts
// 展示视图原始行, 可选列和文件详情.
import type { WatchRow } from "../models/watch.ts";
import { WatchStore } from "../state/watch-store.ts";
import { button, element, link } from "../utils/dom.ts";
import { episodeEditor } from "./episode-editor.ts";

/** 构建视图表格并在数据更新时保留编辑焦点. */
export function watchTable(store: WatchStore): HTMLElement {
  const section = element("section", undefined, "panel table-panel");
  const tools = element("div", undefined, "table-tools");
  const checkbox = element("input");
  checkbox.type = "checkbox";
  checkbox.addEventListener("change", () => { section.classList.toggle("show-extra", checkbox.checked); });
  const label = element("label", "显示附加列");
  label.prepend(checkbox);
  tools.append(label);
  const scroll = element("div", undefined, "table-scroll");
  const table = element("table");
  const head = element("thead");
  const heading = element("tr");
  for (const title of ["状态", "文件名", "剧集 ID", "弹弹播放", "RSS", "种子", "大小", "下载器", "弹幕更新", "链接"]) {
    heading.append(element("th", title));
  }
  for (const title of ["SHA-256", "种子哈希", "路径", "目录", "扩展名", "字节数", "RSS ID", "动画 ID", "Bangumi", "发布", "更新", "描述"]) {
    heading.append(element("th", title, "extra"));
  }
  head.append(heading);
  const body = element("tbody");
  table.append(head, body);
  scroll.append(table);
  section.append(tools, scroll);

  /** 重新绘制表格并恢复输入框的焦点和光标. */
  const render = (): void => {
    const active = document.activeElement;
    const selected = active instanceof HTMLInputElement && active.dataset.editor
      ? { hash: active.dataset.editor, index: [...body.querySelectorAll<HTMLInputElement>("input[data-editor]")]
        .filter((input) => input.dataset.editor === active.dataset.editor).indexOf(active),
      start: active.selectionStart, end: active.selectionEnd } : null;
    body.replaceChildren();
    for (const row of store.page.items) body.append(renderRow(row, store));
    if (!store.page.items.length) {
      const tr = element("tr");
      const td = element("td", store.loading && !store.initialized ? "正在加载..." : "没有符合条件的记录.", "empty");
      td.colSpan = 22;
      tr.append(td);
      body.append(tr);
    }
    if (selected) {
      const target = [...body.querySelectorAll<HTMLInputElement>("input[data-editor]")]
        .filter((input) => input.dataset.editor === selected.hash)[selected.index];
      if (target && !target.disabled) {
        target.focus();
        if (selected.start !== null && selected.end !== null) target.setSelectionRange(selected.start, selected.end);
      }
    }
  };
  store.addEventListener("change", render);
  render();
  return section;
}

/** 创建一条原始视图记录, 不根据文件哈希合并行. */
function renderRow(row: WatchRow, store: WatchStore): HTMLTableRowElement {
  const tr = element("tr");
  const state = element("td", row.file_sha256 ? "可编辑" : "只读", "muted");
  const filename = textCell(row.file_filename ?? row.mikan_torrent_title);
  const editor = element("td");
  editor.append(episodeEditor(row, store));
  const anime = textCell([row.dandanplay_anime_title, row.dandanplay_episode_ep, row.dandanplay_episode_title]
    .filter((value) => value !== null && value !== "").join(" / ") || null);
  const actions = element("td", undefined, "links");
  if (row.file_url) actions.append(link(row.file_url, "文件"));
  if (row.mikan_torrent_page_url) actions.append(link(row.mikan_torrent_page_url, "详情"));
  if (row.mikan_torrent_download_url) actions.append(link(row.mikan_torrent_download_url, "种子"));
  const hash = textCell(row.file_sha256, "extra hash");
  if (row.file_sha256) {
    const value = row.file_sha256;
    const copy = button("复制", () => {
      void navigator.clipboard.writeText(value).then(() => { copy.textContent = "已复制"; })
        .catch(() => { copy.textContent = "复制失败"; });
    });
    hash.append(copy);
  }
  tr.append(state, filename, editor, anime, textCell(row.mikan_rss_title), textCell(row.mikan_torrent_title),
    textCell(row.file_size), textCell(row.in_qb === null ? null : row.in_qb === 1 ? "是" : "否"),
    textCell(row.dandanplay_episode_danmaku_update_at), actions, hash,
    textCell(row.mikan_torrent_hash_v1, "extra"), textCell(row.file_absolute_path, "extra"),
    textCell(row.file_directory, "extra"), textCell(row.file_extension, "extra"),
    textCell(row.mikan_torrent_file_size, "extra"),
    textCell(row.mikan_rss_id?.toString() ?? null, "extra"), textCell(row.ddp_ani_id, "extra"),
    textCell([row.bgm_ani_id, row.bangumi_anime_title].filter((value) => value !== null).join(" / ") || null, "extra"),
    textCell(row.mikan_torrent_pub_at, "extra"), textCell(row.mikan_torrent_update_at, "extra"),
    textCell(row.mikan_torrent_description, "extra"));
  return tr;
}

/** 为长文本提供截断显示和可展开的完整内容. */
function textCell(value: string | null, className = ""): HTMLTableCellElement {
  const cell = element("td", undefined, className);
  const text = element("span", value ?? "-", "truncate");
  if (value && value.length > 60) {
    const details = element("details", undefined, "text-details");
    const summary = element("summary");
    summary.append(text);
    details.append(summary, element("div", value, "full-text"));
    cell.append(details);
  } else {
    cell.append(text);
  }
  return cell;
}
