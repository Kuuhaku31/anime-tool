
// view_watch/src/ui/filter-bar.ts
// 提供搜索, 筛选, 排序和每页数量控件.
import type { SortField, WatchQuery } from "../models/watch.ts";
import { WatchStore } from "../state/watch-store.ts";
import { element } from "../utils/dom.ts";

/** 创建过滤控件并在变化时更新查询. */
export function filterBar(store: WatchStore): HTMLElement {
  const bar = element("div", undefined, "filters");
  const search = element("input");
  search.type = "search";
  search.placeholder = "搜索文件, 路径, 番剧或种子";
  search.setAttribute("aria-label", "搜索");
  const searchMessage = element("span", "", "muted search-message");
  searchMessage.setAttribute("aria-live", "polite");
  const searchKey = "view-watch-search";
  search.value = store.query.search;
  try {
    const saved = localStorage.getItem(searchKey);
    if (saved !== null) {
      search.value = saved;
      store.query.search = saved;
    }
  } catch {
    searchMessage.textContent = "无法读取搜索内容, 使用默认值.";
  }
  let timer = 0;
  search.addEventListener("input", () => {
    try {
      localStorage.setItem(searchKey, search.value);
      searchMessage.textContent = "";
    } catch {
      searchMessage.textContent = "无法保存搜索内容, 设置仅在本次页面中有效.";
    }
    window.clearTimeout(timer);
    timer = window.setTimeout(() => store.setQuery({ search: search.value, page: 0 }), 300);
  });
  bar.append(search, searchMessage);
  bar.append(select("剧集状态", [["all", "全部剧集状态"], ["null", "未识别"], ["unmatchable", "无法关联"], ["assigned", "已设置"]],
    (value) => store.setQuery({ ddp: value as WatchQuery["ddp"], page: 0 })));
  bar.append(select("下载器状态", [["all", "全部下载器状态"], ["yes", "在 qBittorrent"], ["no", "不在 qBittorrent"]],
    (value) => store.setQuery({ qb: value as WatchQuery["qb"], page: 0 })));
  bar.append(select("可编辑状态", [["all", "全部记录"], ["yes", "可编辑"], ["no", "仅只读"]],
    (value) => store.setQuery({ editable: value as WatchQuery["editable"], page: 0 })));
  bar.append(select("排序字段", [["mikan_rss_id", "RSS ID"], ["mikan_torrent_pub_at", "发布时间"],
    ["mikan_torrent_update_at", "更新时间"], ["file_filename", "文件名"], ["mikan_torrent_file_size", "文件大小"],
    ["ddp_epi_id", "剧集 ID"], ["dandanplay_episode_danmaku_update_at", "弹幕更新时间"]],
  (value) => store.setQuery({ sort: value as SortField, page: 0 })));
  bar.append(select("排序方向", [["asc", "升序"], ["desc", "降序"]],
    (value) => store.setQuery({ direction: value as WatchQuery["direction"], page: 0 })));
  const pageSizeMessage = element("span", "", "muted page-size-message");
  pageSizeMessage.setAttribute("aria-live", "polite");
  const pageSizeKey = "view-watch-page-size";
  const pageSize = select("每页数量", [["100", "100 行"], ["20", "20 行"], ["50", "50 行"], ["200", "200 行"],
    ["500", "500 行"], ["1000", "1000 行"]],
    (value) => {
      try {
        localStorage.setItem(pageSizeKey, value);
        pageSizeMessage.textContent = "";
      } catch {
        pageSizeMessage.textContent = "无法保存每页数量, 设置仅在本次页面中有效.";
      }
      store.setQuery({ pageSize: Number(value), page: 0 });
    });
  pageSize.value = String(store.query.pageSize);
  try {
    const saved = localStorage.getItem(pageSizeKey);
    if (saved !== null) {
      if ([...pageSize.options].some((option) => option.value === saved)) {
        pageSize.value = saved;
        store.query.pageSize = Number(saved);
      } else {
        pageSizeMessage.textContent = "已忽略无效的每页数量设置.";
      }
    }
  } catch {
    pageSizeMessage.textContent = "无法读取每页数量设置, 使用默认值.";
  }
  bar.append(pageSize, pageSizeMessage);
  return bar;
}

/** 创建带标签的选择框. */
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
