
// view_watch/src/ui/pagination.ts
// 提供范围显示和页码导航.
import { WatchStore } from "../state/watch-store.ts";
import { button, element } from "../utils/dom.ts";

/** 创建随查询结果同步的分页控件. */
export function pagination(store: WatchStore): HTMLElement {
  const bar = element("nav", undefined, "pagination");
  bar.setAttribute("aria-label", "分页");
  const first = button("首页", () => store.setQuery({ page: 0 }));
  const previous = button("上一页", () => store.setQuery({ page: Math.max(0, store.query.page - 1) }));
  const range = element("span");
  const next = button("下一页", () => store.setQuery({ page: store.query.page + 1 }));
  const last = button("末页", () => store.setQuery({ page: Math.max(0, Math.ceil(store.page.total / store.query.pageSize) - 1) }));
  bar.append(first, previous, range, next, last);
  store.addEventListener("change", () => {
    const start = store.query.page * store.query.pageSize;
    range.textContent = store.page.total ? `${start + 1}-${Math.min(start + store.page.items.length, store.page.total)} / ${store.page.total}` : "0 / 0";
    first.disabled = previous.disabled = store.loading || store.query.page === 0;
    next.disabled = last.disabled = store.loading || start + store.query.pageSize >= store.page.total;
  });
  return bar;
}
