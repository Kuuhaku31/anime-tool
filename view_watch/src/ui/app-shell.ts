
// view_watch/src/ui/app-shell.ts
// 构建管理页面外壳和连接状态.
import { WatchStore } from "../state/watch-store.ts";
import { button, element } from "../utils/dom.ts";

/** 构建页面外壳, 将统计放入工具栏并保持连接状态同步. */
export function appShell(root: HTMLElement, store: WatchStore, toolbar: HTMLElement): HTMLElement {
  const header = element("header");
  const title = element("div");
  title.append(element("h1", "Anime Watch"));
  const nav = element("nav", undefined, "page-nav");
  nav.setAttribute("aria-label", "页面导航");
  const files = element("a", "文件和种子");
  files.href = "/";
  files.setAttribute("aria-current", "page");
  const episodes = element("a", "观看剧集");
  episodes.href = "/episodes";
  nav.append(files, episodes);
  title.append(nav);
  const actions = element("div", undefined, "header-actions");
  const status = element("span", "连接中", "status");
  const refresh = button("刷新", () => { void store.refresh(); });
  const updatedAt = element("span", "最近刷新 -", "muted");
  actions.append(status, refresh, updatedAt);
  header.append(title, actions);
  const main = element("main");
  const summary = element("div", undefined, "summary");
  toolbar.prepend(summary);
  const error = element("div", undefined, "error global-error");
  error.setAttribute("role", "alert");
  const content = element("div", undefined, "content");
  main.append(error, content);
  root.append(header, main);
  store.addEventListener("change", () => {
    const rows = store.page.items;
    status.textContent = store.error ? "连接异常" : store.loading ? "刷新中" : store.initialized ? "已连接" : "连接中";
    updatedAt.textContent = `最近刷新 ${store.updatedAt?.toLocaleTimeString() ?? "-"}`;
    error.textContent = store.error ?? "";
    error.hidden = !store.error;
    summary.textContent = `筛选结果 ${store.page.total} 行 | 本页可编辑 ${rows.filter((row) => row.file_sha256 !== null).length} 行 | 未识别 ${rows.filter((row) => row.file_sha256 !== null && row.ddp_epi_id === null).length} | 无法关联 ${rows.filter((row) => row.ddp_epi_id === "-1").length} | 已设置 ${rows.filter((row) => row.ddp_epi_id !== null && row.ddp_epi_id !== "-1").length}`;
  });
  return content;
}
