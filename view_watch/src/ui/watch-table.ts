
// view_watch/src/ui/watch-table.ts
// 展示视图原始行和文件详情.
import type { WatchRow } from "../models/watch.ts";
import { WatchStore } from "../state/watch-store.ts";
import { element, link } from "../utils/dom.ts";
import { episodeEditor } from "./episode-editor.ts";

/** 构建视图表格及工具栏控件, 并在数据更新时保留编辑焦点. */
export function watchTable(store: WatchStore, controls: HTMLElement): HTMLElement {
  const section = element("section", undefined, "panel table-panel");
  const resizeMessage = element("span", "", "muted resize-message");
  resizeMessage.setAttribute("aria-live", "polite");
  const scroll = element("div", undefined, "table-scroll");
  const table = element("table");
  const head = element("thead");
  const heading = element("tr");
  const names = ["RSS标题", "种子名称", "大小", "文件名", "剧集 ID", "弹弹播放", "下载器状态", "链接"];
  const defaults = [180, 220, 100, 200, 260, 200, 100, 160];
  const storageKey = "view-watch-column-widths-v3";
  const minWidth = 60;
  const maxWidth = 4000;
  const widths = [...defaults];
  try {
    let stored = localStorage.getItem(storageKey);
    let expectedLength = defaults.length;
    let legacyOrder: number[] | null = null;
    // 按新列顺序映射旧版列宽, 保留用户已有的布局设置.
    if (stored === null) {
      stored = localStorage.getItem("view-watch-column-widths-v2");
      if (stored !== null) {
        expectedLength = 21;
        legacyOrder = [0, 1, 2, 3, 6, 7, 4, 5];
      } else {
        stored = localStorage.getItem("view-watch-column-widths-v1");
        expectedLength = 22;
        legacyOrder = [4, 5, 6, 1, 2, 3, 7, 9];
      }
    }
    const saved: unknown = JSON.parse(stored ?? "null");
    if (saved !== null) {
      if (!Array.isArray(saved) || saved.length !== expectedLength
        || !saved.every((width) => typeof width === "number" && Number.isFinite(width)
          && width >= minWidth && width <= maxWidth)) {
        resizeMessage.textContent = "已忽略无效的列宽配置.";
      } else {
        widths.splice(0, widths.length, ...(legacyOrder ? legacyOrder.map((index) => saved[index]) : saved));
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
      total += widths[index];
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
    columns.append(column);
    const cell = element("th");
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
  updateWidths();
  head.append(heading);
  const body = element("tbody");
  table.append(columns, head, body);
  scroll.append(table);
  controls.append(resizeMessage);
  section.append(scroll);

  /** 重新绘制表格并恢复输入框的焦点和光标. */
  const render = (): void => {
    const active = document.activeElement;
    const selected = active instanceof HTMLInputElement && active.dataset.editor
      ? { hash: active.dataset.editor, index: [...body.querySelectorAll<HTMLInputElement>("input[data-editor]")]
        .filter((input) => input.dataset.editor === active.dataset.editor).indexOf(active),
      start: active.selectionStart, end: active.selectionEnd } : null;
    const selectedDetails = active instanceof HTMLTableRowElement ? active.dataset.detailsKey : undefined;
    body.replaceChildren();
    const occurrences = new Map<string, number>();
    for (const row of store.page.items) {
      const identity = JSON.stringify([row.file_sha256, row.file_absolute_path, row.mikan_torrent_hash_v1, row.mikan_rss_id]);
      const occurrence = occurrences.get(identity) ?? 0;
      occurrences.set(identity, occurrence + 1);
      const key = JSON.stringify([identity, occurrence]);
      const tr = renderRow(row, store);
      tr.tabIndex = 0;
      tr.dataset.detailsKey = key;
      /** 展开或收起当前记录, 不改变其他同文件记录的展开状态. */
      const toggleDetails = (): void => {
        if (expandedRows.has(key)) {
          expandedRows.delete(key);
          const details = tr.nextElementSibling;
          if (details?.classList.contains("record-details")) details.remove();
        } else {
          expandedRows.add(key);
          tr.after(renderDetails(row, names.length));
        }
        updateToggle();
      };
      /** 同步行的展开状态和操作提示. */
      const updateToggle = (): void => {
        const expanded = expandedRows.has(key);
        tr.setAttribute("aria-expanded", String(expanded));
        tr.title = `${expanded ? "收起" : "展开"}记录详情`;
      };
      tr.addEventListener("click", (event) => {
        if (event.target instanceof Element
          && event.target.closest("a, button, input, select, textarea, [aria-disabled='true']")) return;
        const selection = window.getSelection();
        if (selection && !selection.isCollapsed && selection.containsNode(tr, true)) return;
        toggleDetails();
      });
      tr.addEventListener("keydown", (event) => {
        if (event.target !== tr || (event.key !== "Enter" && event.key !== " ")) return;
        event.preventDefault();
        toggleDetails();
      });
      updateToggle();
      body.append(tr);
      if (expandedRows.has(key)) body.append(renderDetails(row, names.length));
    }
    if (!store.page.items.length) {
      const tr = element("tr");
      const td = element("td", store.loading && !store.initialized ? "正在加载..." : "没有符合条件的记录.", "empty");
      td.colSpan = names.length;
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
      const target = [...body.querySelectorAll<HTMLTableRowElement>("tr[data-details-key]")]
        .find((row) => row.dataset.detailsKey === selectedDetails);
      target?.focus({ preventScroll: true });
    }
  };
  store.addEventListener("change", render);
  render();
  return section;
}

/** 创建一条原始视图记录, 不根据文件哈希合并行. */
function renderRow(row: WatchRow, store: WatchStore): HTMLTableRowElement {
  const tr = element("tr", undefined, "watch-row");
  const filename = textCell(row.file_filename);
  const editor = element("td");
  editor.append(episodeEditor(row, store));
  const anime = textCell([row.dandanplay_anime_title, row.dandanplay_episode_ep, row.dandanplay_episode_title]
    .filter((value) => value !== null && value !== "").join(" / ") || null);
  const actions = element("td");
  const links = element("div", undefined, "links");
  links.append(optionalLink(row.file_url, "文件"), optionalLink(row.mikan_torrent_page_url, "详情"),
    optionalLink(row.mikan_torrent_download_url, "种子"));
  actions.append(links);
  tr.append(textCell(row.mikan_rss_title), textCell(row.mikan_torrent_title), textCell(row.file_size), filename,
    editor, anime, textCell(row.in_qb === null ? null : row.in_qb === 1 ? "是" : "否"), actions);
  return tr;
}

/** 始终显示链接名称, 缺失或不安全的地址不可点击. */
function optionalLink(url: string | null, label: string): HTMLElement {
  const target = url ? link(url, label) : element("span", label);
  if (target instanceof HTMLAnchorElement) {
    target.title = url ?? "";
  } else {
    target.textContent = label;
    target.classList.add("link-disabled");
    target.setAttribute("aria-disabled", "true");
    target.title = url ? "链接地址无效" : "链接不可用";
  }
  return target;
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
function textCell(value: string | null): HTMLTableCellElement {
  const cell = element("td");
  const text = element("span", value ?? "-", "truncate");
  if (value !== null) text.title = value;
  cell.append(text);
  return cell;
}
