
// view_watch/src/ui/watch-episode-page.ts
// 构建观看剧集页面, 查询控件, 表格和批量编辑界面.
import type { EpisodeQuery, WatchEpisodeRow } from "../models/watch-episode.ts";
import { localDateTime, WatchEpisodeStore } from "../state/watch-episode-store.ts";
import { button, element } from "../utils/dom.ts";
import { pagination } from "./pagination.ts";

/** 构建独立的观看剧集页面并启动首次查询. */
export function episodePage(root: HTMLElement): void {
  const store = new WatchEpisodeStore();
  const header = element("header");
  const title = element("div");
  title.append(element("h1", "Anime Watch"));
  const nav = element("nav", undefined, "page-nav");
  nav.setAttribute("aria-label", "页面导航");
  const home = element("a", "文件和种子");
  home.href = "/";
  const current = element("a", "观看剧集");
  current.href = "/episodes";
  current.setAttribute("aria-current", "page");
  nav.append(home, current);
  title.append(nav);
  const actions = element("div", undefined, "header-actions");
  const status = element("span", "连接中", "status");
  const refresh = button("刷新", () => { void store.refresh(); });
  const updated = element("span", "最近刷新 -", "muted");
  actions.append(status, refresh, updated);
  header.append(title, actions);
  const main = element("main");
  const error = element("div", "", "error global-error");
  error.setAttribute("role", "alert");
  const content = element("div", undefined, "content");
  const toolbar = element("section", undefined, "toolbar panel");
  const summary = element("div", "", "summary");
  const controls = element("div", undefined, "toolbar-controls");
  toolbar.append(summary, controls);
  controls.append(episodeFilters(store), pagination(store));
  const table = episodeTable(store, controls);
  content.append(toolbar, table);
  main.append(error, content);
  root.append(header, main);
  store.addEventListener("change", () => {
    status.textContent = store.error ? "连接异常" : store.loading ? "刷新中" : store.initialized ? "已连接" : "连接中";
    updated.textContent = `最近刷新 ${store.updatedAt?.toLocaleTimeString() ?? "-"}`;
    error.textContent = store.error ?? "";
    error.hidden = !store.error;
    summary.textContent = `筛选结果 ${store.page.total} 行 | 本页已观看 ${store.page.items.filter((row) => row.watched_at !== null).length} 行 | 未观看 ${store.page.items.filter((row) => row.watched_at === null).length} 行`;
  });
  void store.refresh();
}

/** 构造搜索, 状态筛选, 排序和分页大小控件. */
function episodeFilters(store: WatchEpisodeStore): HTMLElement {
  const filters = element("div", undefined, "filters");
  const search = element("input");
  search.type = "search";
  search.placeholder = "搜索番剧, 剧集或 ID";
  search.setAttribute("aria-label", "搜索观看剧集");
  const message = element("span", "", "muted");
  message.setAttribute("aria-live", "polite");
  const searchKey = "view-watch-episode-search";
  const pageSizeKey = "view-watch-episode-page-size";
  try {
    search.value = localStorage.getItem(searchKey) ?? "";
    store.query.search = search.value;
  } catch {
    message.textContent = "无法读取搜索设置.";
  }
  let timer = 0;
  search.addEventListener("input", () => {
    try { localStorage.setItem(searchKey, search.value); message.textContent = ""; }
    catch { message.textContent = "无法保存搜索内容, 设置仅在本次页面有效."; }
    window.clearTimeout(timer);
    timer = window.setTimeout(() => store.setQuery({ search: search.value, page: 0 }), 300);
  });
  filters.append(search);
  filters.append(select("观看状态", [["all", "全部状态"], ["yes", "已观看"], ["no", "未观看"], ["aired-no", "已放送但未观看"]],
    (value) => store.setQuery({ watched: value as EpisodeQuery["watched"], page: 0 })));
  filters.append(select("评分筛选", [["", "全部评分"], ["null", "无评分"], ...[1, 2, 3, 4, 5].map((n) => [String(n), `${n} 分`])],
    (value) => store.setQuery({ rating: value, page: 0 })));
  filters.append(select("排序字段", [["bgm_epi_id", "剧集 ID"], ["bgm_ani_id", "动画 ID"], ["bgm_ani_title", "动画标题"],
    ["bgm_epi_ep", "剧集序号"], ["bgm_epi_airdate", "放送日期"], ["watched_at", "观看时间"], ["rating", "评分"]],
  (value) => store.setQuery({ sort: value as EpisodeQuery["sort"], page: 0 })));
  filters.append(select("排序方向", [["asc", "升序"], ["desc", "降序"]],
    (value) => store.setQuery({ direction: value as EpisodeQuery["direction"], page: 0 })));
  const sizes = [20, 50, 100, 200, 500, 1000];
  const pageSize = select("每页数量", sizes.map((n) => [String(n), `${n} 行`]), (value) => {
    try { localStorage.setItem(pageSizeKey, value); message.textContent = ""; }
    catch { message.textContent = "无法保存每页数量, 设置仅在本次页面有效."; }
    store.setQuery({ pageSize: Number(value), page: 0 });
  });
  try {
    const saved = localStorage.getItem(pageSizeKey);
    if (saved !== null && sizes.includes(Number(saved))) store.query.pageSize = Number(saved);
    else if (saved !== null) message.textContent = "已忽略无效的每页数量设置.";
  } catch { message.textContent = "无法读取每页数量设置."; }
  pageSize.value = String(store.query.pageSize);
  filters.append(pageSize, message);
  return filters;
}

/** 构建可调整列宽和跨页选择的剧集表格. */
function episodeTable(store: WatchEpisodeStore, controls: HTMLElement): HTMLElement {
  const section = element("section", undefined, "panel table-panel episode-table-panel");
  const scroll = element("div", undefined, "table-scroll");
  const table = element("table");
  const columns = element("colgroup");
  const head = element("thead");
  const heading = element("tr");
  const body = element("tbody");
  const names = ["选择", "bgm_epi_id", "bgm_ani_id", "ddp_ani_id", "mikan_rss_id", "mikan_rss_title", "bgm_ani_title",
    "bgm_epi_ep", "bgm_epi_title", "放送日期", "星期", "放送状态", "watched_at", "rating", "操作"];
  const defaults = [55, 120, 120, 140, 130, 240, 240, 110, 240, 120, 85, 100, 235, 100, 230];
  const widths = [...defaults];
  const storageKey = "view-watch-episode-column-widths-v3";
  const resizeMessage = element("span", "", "muted");
  resizeMessage.setAttribute("aria-live", "polite");
  try {
    const saved: unknown = JSON.parse(localStorage.getItem(storageKey) ?? "null");
    if (saved !== null) {
      if (!Array.isArray(saved) || saved.length !== widths.length || !saved.every((n) =>
        typeof n === "number" && Number.isFinite(n) && n >= 50 && n <= 4000)) {
        resizeMessage.textContent = "已忽略无效的列宽配置.";
      } else widths.splice(0, widths.length, ...saved);
    }
  } catch { resizeMessage.textContent = "无法读取列宽设置."; }
  const handles: HTMLElement[] = [];
  /** 同步每列宽度和表格总宽度. */
  const updateWidths = (): void => {
    let total = 0;
    widths.forEach((width, index) => {
      const col = columns.children[index] as HTMLTableColElement;
      col.style.width = `${width}px`;
      col.hidden = index === 0 && !store.selectionEnabled;
      if (index !== 0 || store.selectionEnabled) total += width;
      handles[index].setAttribute("aria-valuenow", String(Math.round(width)));
    });
    table.style.width = `${total}px`;
    (heading.children[0] as HTMLElement).hidden = !store.selectionEnabled;
  };
  /** 保存列宽, 失败时明确提示. */
  const saveWidths = (): void => {
    try { localStorage.setItem(storageKey, JSON.stringify(widths)); resizeMessage.textContent = ""; }
    catch { resizeMessage.textContent = "无法保存列宽, 设置仅在本次页面有效."; }
  };
  names.forEach((name, index) => {
    columns.append(element("col"));
    const th = element("th");
    th.scope = "col";
    th.append(element("span", name, "column-title"));
    const handle = element("span", undefined, "column-resizer");
    handle.tabIndex = 0;
    handle.setAttribute("role", "separator");
    handle.setAttribute("aria-orientation", "vertical");
    handle.setAttribute("aria-label", `调整${name}列宽`);
    handle.setAttribute("aria-valuemin", "50");
    handle.setAttribute("aria-valuemax", "4000");
    /** 当前拖动的指针及起始位置. */
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
      widths[index] = Math.max(50, Math.min(4000, Math.round(drag.width + event.clientX - drag.x)));
      updateWidths();
    });
    handle.addEventListener("pointerup", (event) => {
      if (!drag || drag.pointerId !== event.pointerId) return;
      drag = null;
      saveWidths();
    });
    handle.addEventListener("pointercancel", () => {
      if (!drag) return;
      widths[index] = drag.width;
      drag = null;
      updateWidths();
    });
    handle.addEventListener("dblclick", () => { widths[index] = defaults[index]; updateWidths(); saveWidths(); });
    handle.addEventListener("keydown", (event) => {
      if (!["ArrowLeft", "ArrowRight", "Home"].includes(event.key)) return;
      event.preventDefault();
      const step = event.shiftKey ? 50 : 10;
      widths[index] = event.key === "Home" ? defaults[index]
        : Math.max(50, Math.min(4000, widths[index] + (event.key === "ArrowRight" ? step : -step)));
      updateWidths();
      saveWidths();
    });
    handles.push(handle);
    th.append(handle);
    heading.append(th);
  });
  head.append(heading);
  table.append(columns, head, body);
  scroll.append(table);
  section.append(scroll);
  const bulk = element("div", undefined, "episode-bulk");
  const label = element("label", undefined, "selection-control");
  const enabled = element("input");
  enabled.type = "checkbox";
  enabled.addEventListener("change", () => store.setSelectionEnabled(enabled.checked));
  label.append(enabled, document.createTextNode("启用多选"));
  const count = element("span");
  const mark = button("批量标记已观看", () => { void store.bulk(false); });
  const remove = button("批量删除观看记录", () => { void store.bulk(true); });
  const feedback = element("span", "", "muted");
  feedback.setAttribute("role", "status");
  const legend = element("span", undefined, "episode-legend");
  legend.append(element("span", "已观看", "watched-legend"), element("span", "未观看", "unwatched-legend"));
  bulk.append(label, count, mark, remove, feedback, resizeMessage, legend);
  controls.append(bulk);

  /** 重绘行内容, 恢复正在编辑的输入焦点. */
  const render = (): void => {
    const active = document.activeElement;
    const focusKey = active instanceof HTMLElement ? active.dataset.focusKey : undefined;
    body.replaceChildren();
    for (const row of store.page.items) body.append(renderRow(row, store));
    if (!store.page.items.length) {
      const tr = element("tr");
      const td = element("td", store.loading && !store.initialized ? "正在加载..." : "没有符合条件的记录.", "empty");
      td.colSpan = names.length - Number(!store.selectionEnabled);
      tr.append(td);
      body.append(tr);
    }
    if (focusKey) {
      const target = [...body.querySelectorAll<HTMLElement>("[data-focus-key]")].find((item) => item.dataset.focusKey === focusKey);
      if (target && !(target instanceof HTMLInputElement && target.disabled)) target.focus({ preventScroll: true });
    }
    const watched = [...store.selected.values()].filter(Boolean).length;
    const unwatched = store.selected.size - watched;
    count.textContent = store.selectionEnabled ? `已选 ${store.selected.size} | 可创建 ${unwatched} | 可删除 ${watched}` : "";
    mark.hidden = remove.hidden = !store.selectionEnabled;
    mark.disabled = store.bulkSaving || store.loading || unwatched === 0 || [...store.drafts.values()].some((draft) => draft.saving);
    remove.disabled = store.bulkSaving || store.loading || watched === 0 || [...store.drafts.values()].some((draft) => draft.saving);
    enabled.disabled = store.bulkSaving;
    feedback.textContent = store.message;
    updateWidths();
  };
  store.addEventListener("change", render);
  render();
  return section;
}

/** 绘制一行并将重复剧集共享的草稿关联到输入框. */
function renderRow(row: WatchEpisodeRow, store: WatchEpisodeStore): HTMLTableRowElement {
  const tr = element("tr", undefined, row.watched_at === null ? "episode-unwatched" : "episode-watched");
  const id = row.bgm_epi_id;
  if (store.selectionEnabled) {
    const selection = element("td", undefined, "selection-cell");
    const checkbox = element("input");
    checkbox.type = "checkbox";
    checkbox.checked = id !== null && store.selected.has(id);
    checkbox.disabled = id === null || store.loading || store.bulkSaving;
    checkbox.setAttribute("aria-label", `选择剧集 ${id ?? "未知"}`);
    checkbox.dataset.focusKey = `select-${id}`;
    checkbox.addEventListener("change", () => store.select(row, checkbox.checked));
    selection.append(checkbox);
    tr.append(selection);
  }
  for (const value of [row.bgm_epi_id, row.bgm_ani_id, row.ddp_ani_id, row.mikan_rss_id,
    row.mikan_rss_title, row.bgm_ani_title, row.bgm_epi_ep, row.bgm_epi_title]) {
    const td = element("td");
    const text = value === null ? "-" : String(value);
    const span = element("span", text, "truncate");
    span.title = text;
    td.append(span);
    tr.append(td);
  }
  const airdate = row.bgm_epi_airdate;
  const today = localDateTime(new Date().toISOString()).slice(0, 10);
  const broadcast = !airdate ? "unknown" : airdate < today ? "aired" : airdate === today ? "today" : "upcoming";
  const labels = { unknown: "日期未知", aired: "已放送", today: "今天放送", upcoming: "未放送" };
  const weekday = row.bgm_epi_weekday;
  const weekdayText = weekday !== null && weekday >= 1 && weekday <= 7
    ? ["星期一", "星期二", "星期三", "星期四", "星期五", "星期六", "星期日"][weekday - 1] : "-";
  for (const text of [airdate ?? "-", weekdayText, labels[broadcast]]) {
    const cell = element("td", undefined, `broadcast-${broadcast}`);
    cell.append(element("span", text, "broadcast-label"));
    tr.append(cell);
  }
  const state = id === null ? undefined : store.drafts.get(id);
  const timeCell = element("td", undefined, "episode-editor");
  const time = element("input");
  time.type = "datetime-local";
  time.step = "1";
  time.value = state?.time ?? localDateTime(row.watched_at);
  time.disabled = id === null || !!state?.saving || store.bulkSaving;
  time.setAttribute("aria-label", `剧集 ${id ?? "未知"} 的观看时间`);
  time.dataset.focusKey = `time-${id}`;
  time.addEventListener("input", () => {
    const draft = store.draft(row);
    draft.time = time.value;
    draft.timeChanged = true;
    draft.dirty = true;
    draft.error = null;
    for (const other of document.querySelectorAll<HTMLInputElement>("input[data-focus-key]")) {
      if (other !== time && other.dataset.focusKey === time.dataset.focusKey) other.value = time.value;
    }
  });
  timeCell.append(time);
  const ratingCell = element("td", undefined, "episode-editor");
  const rating = select(`剧集 ${id ?? "未知"} 的评分`, [["", "无评分"], ...[1, 2, 3, 4, 5].map((n) => [String(n), `${n} 分`])],
    (value) => {
      const draft = store.draft(row);
      draft.rating = value;
      draft.dirty = true;
      draft.error = null;
      for (const other of document.querySelectorAll<HTMLSelectElement>("select[data-focus-key]")) {
        if (other !== rating && other.dataset.focusKey === rating.dataset.focusKey) other.value = value;
      }
    });
  rating.value = state?.rating ?? (row.rating === null ? "" : String(row.rating));
  rating.disabled = id === null || !!state?.saving || store.bulkSaving;
  rating.dataset.focusKey = `rating-${id}`;
  ratingCell.append(rating);
  const actionCell = element("td");
  const actions = element("div", undefined, "episode-actions");
  const save = button(state?.saving ? "保存中" : "保存", () => { void store.save(row); }, "primary");
  const cancel = button("取消", () => { if (id !== null) store.cancel(id); });
  const watched = button("已观看", () => { void store.markWatched(row); });
  const remove = button("删除", () => { void store.deleteRow(row); });
  for (const item of [save, cancel, watched, remove]) item.disabled = id === null || !!state?.saving || store.bulkSaving;
  watched.hidden = row.watched_at !== null;
  remove.hidden = row.watched_at === null;
  actions.append(save, cancel, watched, remove);
  actionCell.append(actions);
  if (state?.error) {
    const error = element("span", state.error, "error");
    error.setAttribute("role", "alert");
    actionCell.append(error);
  }
  tr.append(timeCell, ratingCell, actionCell);
  return tr;
}

/** 创建带无障碍标签的选择控件. */
function select(label: string, options: string[][], change: (value: string) => void): HTMLSelectElement {
  const node = element("select");
  node.setAttribute("aria-label", label);
  for (const [value, text] of options) {
    const option = element("option", text);
    option.value = value;
    node.append(option);
  }
  node.addEventListener("change", () => change(node.value));
  return node;
}
