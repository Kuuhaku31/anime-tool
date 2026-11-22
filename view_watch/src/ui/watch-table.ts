
// view_watch/src/ui/watch-table.ts
// 展示视图原始行, 可选列和文件详情.
import type { WatchRow } from "../models/watch.ts";
import { WatchStore } from "../state/watch-store.ts";
import { button, element, link } from "../utils/dom.ts";
import { episodeEditor } from "./episode-editor.ts";

/** 构建视图表格及工具栏控件, 并在数据更新时保留编辑焦点. */
export function watchTable(store: WatchStore, controls: HTMLElement): HTMLElement {
  const section = element("section", undefined, "panel table-panel");
  const tools = element("div", undefined, "table-tools");
  const checkbox = element("input");
  checkbox.type = "checkbox";
  const label = element("label", "显示附加列");
  label.prepend(checkbox);
  const resizeMessage = element("span", "", "muted resize-message");
  resizeMessage.setAttribute("aria-live", "polite");
  tools.append(label);
  const scroll = element("div", undefined, "table-scroll");
  const table = element("table");
  const head = element("thead");
  const heading = element("tr");
  const names = ["状态", "文件名", "剧集 ID", "弹弹播放", "RSS", "种子", "大小", "下载器", "弹幕更新", "链接",
    "SHA-256", "种子哈希", "路径", "目录", "扩展名", "字节数", "RSS ID", "动画 ID", "Bangumi", "发布", "更新", "描述"];
  const defaults = [100, 200, 260, 200, 180, 220, 100, 100, 160, 160,
    240, 200, 240, 200, 100, 120, 100, 120, 180, 140, 140, 240];
  const storageKey = "view-watch-column-widths-v1";
  const minWidth = 60;
  const maxWidth = 4000;
  const widths = [...defaults];
  try {
    const saved: unknown = JSON.parse(localStorage.getItem(storageKey) ?? "null");
    if (saved !== null) {
      if (!Array.isArray(saved) || saved.length !== defaults.length
        || !saved.every((width) => typeof width === "number" && Number.isFinite(width)
          && width >= minWidth && width <= maxWidth)) {
        resizeMessage.textContent = "已忽略无效的列宽配置.";
      } else {
        widths.splice(0, widths.length, ...saved);
      }
    }
  } catch {
    resizeMessage.textContent = "无法读取列宽配置, 使用默认列宽.";
  }
  const columns = element("colgroup");
  const handles: HTMLElement[] = [];
  const expandedRows = new Set<string>();

  /** 同步每列的尺寸和表格总宽度, 保持横向滚动可用. */
  const updateWidths = (): void => {
    let total = 0;
    for (let index = 0; index < names.length; index++) {
      const column = columns.children[index] as HTMLTableColElement;
      column.style.width = `${widths[index]}px`;
      handles[index].setAttribute("aria-valuenow", String(Math.round(widths[index])));
      handles[index].setAttribute("aria-valuetext", `${Math.round(widths[index])} 像素`);
      if (index < 10 || checkbox.checked) total += widths[index];
    }
    table.style.width = `${total}px`;
  };

  /** 保存列宽, 浏览器不允许保存时显示明确提示. */
  const saveWidths = (): void => {
    try {
      localStorage.setItem(storageKey, JSON.stringify(widths));
      resizeMessage.textContent = "";
    } catch {
      resizeMessage.textContent = "无法保存列宽, 设置仅在本次页面中有效.";
    }
  };

  for (let index = 0; index < names.length; index++) {
    const column = element("col");
    if (index >= 10) column.className = "extra";
    columns.append(column);
    const cell = element("th", undefined, index >= 10 ? "extra" : undefined);
    cell.scope = "col";
    cell.append(element("span", names[index], "column-title"));
    const handle = element("span", undefined, "column-resizer");
    handle.tabIndex = 0;
    handle.setAttribute("role", "separator");
    handle.setAttribute("aria-orientation", "vertical");
    handle.setAttribute("aria-label", `调整${names[index]}列宽`);
    handle.setAttribute("aria-valuemin", String(minWidth));
    handle.setAttribute("aria-valuemax", String(maxWidth));
    handle.title = "拖动调整列宽, 双击恢复默认";
    /** 记录当前指针拖动的起始状态. */
    let drag: {
      /** 正在拖动的指针标识. */ pointerId: number;
      /** 拖动起始水平坐标. */ x: number;
      /** 拖动起始列宽. */ width: number;
    } | null = null;
    handle.addEventListener("pointerdown", (event) => {
      if (event.button !== 0) return;
      event.preventDefault();
      drag = { pointerId: event.pointerId, x: event.clientX, width: widths[index] };
      handle.setPointerCapture(event.pointerId);
    });
    handle.addEventListener("pointermove", (event) => {
      if (!drag || drag.pointerId !== event.pointerId) return;
      widths[index] = Math.max(minWidth, Math.min(maxWidth, Math.round(drag.width + event.clientX - drag.x)));
      updateWidths();
    });
    handle.addEventListener("pointerup", (event) => {
      if (!drag || drag.pointerId !== event.pointerId) return;
      drag = null;
      saveWidths();
    });
    handle.addEventListener("pointercancel", (event) => {
      if (!drag || drag.pointerId !== event.pointerId) return;
      widths[index] = drag.width;
      drag = null;
      updateWidths();
    });
    handle.addEventListener("dblclick", () => {
      widths[index] = defaults[index];
      updateWidths();
      saveWidths();
    });
    handle.addEventListener("keydown", (event) => {
      if (event.key !== "ArrowLeft" && event.key !== "ArrowRight" && event.key !== "Home") return;
      event.preventDefault();
      const step = event.shiftKey ? 50 : 10;
      widths[index] = event.key === "Home" ? defaults[index]
        : Math.max(minWidth, Math.min(maxWidth, widths[index] + (event.key === "ArrowRight" ? step : -step)));
      updateWidths();
      saveWidths();
    });
    handles.push(handle);
    cell.append(handle);
    heading.append(cell);
  }
  checkbox.addEventListener("change", () => {
    section.classList.toggle("show-extra", checkbox.checked);
    updateWidths();
    for (const cell of body.querySelectorAll<HTMLTableCellElement>(".empty, .record-details-cell")) {
      cell.colSpan = checkbox.checked ? names.length : 10;
    }
  });
  tools.append(button("重置列宽", () => {
    widths.splice(0, widths.length, ...defaults);
    updateWidths();
    saveWidths();
  }), resizeMessage);
  updateWidths();
  head.append(heading);
  const body = element("tbody");
  table.append(columns, head, body);
  scroll.append(table);
  controls.append(tools);
  section.append(scroll);

  /** 重新绘制表格并恢复输入框的焦点和光标. */
  const render = (): void => {
    const active = document.activeElement;
    const selected = active instanceof HTMLInputElement && active.dataset.editor
      ? { hash: active.dataset.editor, index: [...body.querySelectorAll<HTMLInputElement>("input[data-editor]")]
        .filter((input) => input.dataset.editor === active.dataset.editor).indexOf(active),
      start: active.selectionStart, end: active.selectionEnd } : null;
    const selectedDetails = active instanceof HTMLButtonElement ? active.dataset.detailsKey : undefined;
    body.replaceChildren();
    const occurrences = new Map<string, number>();
    for (const row of store.page.items) {
      const identity = JSON.stringify([row.file_sha256, row.file_absolute_path, row.mikan_torrent_hash_v1, row.mikan_rss_id]);
      const occurrence = occurrences.get(identity) ?? 0;
      occurrences.set(identity, occurrence + 1);
      const key = JSON.stringify([identity, occurrence]);
      const tr = renderRow(row, store);
      const state = tr.firstElementChild as HTMLTableCellElement;
      const stateLabel = row.file_sha256 ? "可编辑" : "只读";
      const toggle = button("", () => {
        if (expandedRows.has(key)) {
          expandedRows.delete(key);
          const details = tr.nextElementSibling;
          if (details?.classList.contains("record-details")) details.remove();
        } else {
          expandedRows.add(key);
          tr.after(renderDetails(row, checkbox.checked ? names.length : 10));
        }
        updateToggle();
      }, "details-toggle");
      toggle.dataset.detailsKey = key;
      /** 同步详情按钮的显示状态和无障碍标签. */
      const updateToggle = (): void => {
        const expanded = expandedRows.has(key);
        toggle.textContent = `${stateLabel} ${expanded ? "▾" : "▸"}`;
        toggle.setAttribute("aria-expanded", String(expanded));
        toggle.setAttribute("aria-label", `${expanded ? "收起" : "展开"}记录详情, ${stateLabel}`);
        toggle.title = `${expanded ? "收起" : "展开"}记录详情`;
      };
      updateToggle();
      state.append(toggle);
      body.append(tr);
      if (expandedRows.has(key)) body.append(renderDetails(row, checkbox.checked ? names.length : 10));
    }
    if (!store.page.items.length) {
      const tr = element("tr");
      const td = element("td", store.loading && !store.initialized ? "正在加载..." : "没有符合条件的记录.", "empty");
      td.colSpan = checkbox.checked ? names.length : 10;
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
    } else if (selectedDetails) {
      const target = [...body.querySelectorAll<HTMLButtonElement>(".details-toggle")]
        .find((button) => button.dataset.detailsKey === selectedDetails);
      target?.focus({ preventScroll: true });
    }
  };
  store.addEventListener("change", render);
  render();
  return section;
}

/** 创建一条原始视图记录, 不根据文件哈希合并行. */
function renderRow(row: WatchRow, store: WatchStore): HTMLTableRowElement {
  const tr = element("tr");
  const state = element("td");
  const filename = textCell(row.file_filename ?? row.mikan_torrent_title);
  const editor = element("td");
  editor.append(episodeEditor(row, store));
  const anime = textCell([row.dandanplay_anime_title, row.dandanplay_episode_ep, row.dandanplay_episode_title]
    .filter((value) => value !== null && value !== "").join(" / ") || null);
  const actions = element("td");
  const links = element("div", undefined, "links");
  if (row.file_url) {
    const target = link(row.file_url, "文件");
    target.title = row.file_url;
    links.append(target);
  }
  if (row.mikan_torrent_page_url) {
    const target = link(row.mikan_torrent_page_url, "详情");
    target.title = row.mikan_torrent_page_url;
    links.append(target);
  }
  if (row.mikan_torrent_download_url) {
    const target = link(row.mikan_torrent_download_url, "种子");
    target.title = row.mikan_torrent_download_url;
    links.append(target);
  }
  actions.append(links);
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

/** 显示记录的所有原始字段, 包括默认隐藏的附加信息. */
function renderDetails(row: WatchRow, columnCount: number): HTMLTableRowElement {
  const tr = element("tr", undefined, "record-details");
  const cell = element("td", undefined, "record-details-cell");
  cell.colSpan = columnCount;
  const list = element("dl", undefined, "details-grid");
  for (const [name, value] of Object.entries(row)) {
    const item = element("div", undefined, "details-item");
    const text = value === null ? "-" : String(value);
    item.append(element("dt", name), element("dd", text));
    list.append(item);
  }
  cell.append(list);
  tr.append(cell);
  return tr;
}

/** 截断表格文本并通过悬浮提示显示完整内容. */
function textCell(value: string | null, className = ""): HTMLTableCellElement {
  const cell = element("td", undefined, className);
  const text = element("span", value ?? "-", "truncate");
  if (value !== null) text.title = value;
  cell.append(text);
  return cell;
}
