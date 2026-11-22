
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
  let timer = 0;
  search.addEventListener("input", () => {
    window.clearTimeout(timer);
    timer = window.setTimeout(() => store.setQuery({ search: search.value, page: 0 }), 300);
  });
  bar.append(search);
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
  bar.append(select("每页数量", [["100", "100 行"], ["20", "20 行"], ["50", "50 行"], ["200", "200 行"],
    ["500", "500 行"], ["1000", "1000 行"]],
    (value) => store.setQuery({ pageSize: Number(value), page: 0 })));
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
