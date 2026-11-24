
// view_watch/src/main.ts
// 组装视图管理界面并启动数据刷新.
import "../style.css";
import { WatchStore } from "./state/watch-store.ts";
import { appShell } from "./ui/app-shell.ts";
import { filterBar } from "./ui/filter-bar.ts";
import { pagination } from "./ui/pagination.ts";
import { watchTable } from "./ui/watch-table.ts";
import { element } from "./utils/dom.ts";
import { episodePage } from "./ui/watch-episode-page.ts";

const root = document.querySelector<HTMLElement>("#app");
if (!root) throw new Error("缺少应用挂载节点.");
if (location.pathname === "/episodes" || location.pathname === "/episodes/") {
  episodePage(root);
} else if (location.pathname === "/") {
const store = new WatchStore();
const toolbar = element("section", undefined, "toolbar panel");
const controls = element("div", undefined, "toolbar-controls");
toolbar.append(controls);
const content = appShell(root, store, toolbar);
controls.append(filterBar(store));
content.append(toolbar, watchTable(store, controls));
controls.append(pagination(store));
store.start();
} else {
  root.append(element("p", "页面不存在."), Object.assign(element("a", "返回首页"), { href: "/" }));
}
