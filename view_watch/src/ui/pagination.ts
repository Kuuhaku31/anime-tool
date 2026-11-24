
// view_watch/src/ui/pagination.ts
// 提供范围显示和页码导航.
import { button, element } from "../utils/dom.ts";

/** 分页控件需要的查询和事件接口. */
interface PaginationStore extends EventTarget {
  /** 当前分页参数. */ query: { page: number; pageSize: number };
  /** 当前页和筛选后的总数. */ page: { items: unknown[]; total: number };
  /** 是否正在加载. */ loading: boolean;
  /** 更新当前页. */ setQuery(change: { page: number }): void;
}

/** 创建随查询结果同步的分页控件. */
export function pagination(store: PaginationStore): HTMLElement {
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
