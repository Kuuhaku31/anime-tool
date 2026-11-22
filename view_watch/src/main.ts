
// view_watch/src/main.ts
// 组装视图管理界面并启动数据刷新.
import "../style.css";
import { WatchStore } from "./state/watch-store.ts";
import { appShell } from "./ui/app-shell.ts";
import { filterBar } from "./ui/filter-bar.ts";
import { pagination } from "./ui/pagination.ts";
import { watchTable } from "./ui/watch-table.ts";
import { element } from "./utils/dom.ts";

const root = document.querySelector<HTMLElement>("#app");
if (!root) throw new Error("缺少应用挂载节点.");
const store = new WatchStore();
const toolbar = element("section", undefined, "toolbar panel");
const controls = element("div", undefined, "toolbar-controls");
toolbar.append(controls);
const content = appShell(root, store, toolbar);
controls.append(filterBar(store));
content.append(toolbar, watchTable(store, controls));
controls.append(pagination(store));
store.start();
