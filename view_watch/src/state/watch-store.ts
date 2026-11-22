
// view_watch/src/state/watch-store.ts
// 管理视图查询, 行内编辑和定时刷新.
import { PostgrestClient } from "../api/postgrest-client.ts";
import type { EditorState, WatchPage, WatchQuery } from "../models/watch.ts";
import { parseEpisodeId } from "../utils/bigint.ts";

/** 管理视图查询和文件关联编辑状态. */
export class WatchStore extends EventTarget {
  /** 当前筛选和分页. */
  query: WatchQuery = { search: "", ddp: "all", qb: "all", editable: "all",
    sort: "mikan_rss_id", direction: "asc", page: 0, pageSize: 100 };
  /** 当前查询结果. */
  page: WatchPage = { items: [], total: 0 };
  /** 当前全局错误. */
  error: string | null = null;
  /** 是否正在查询. */
  loading = false;
  /** 最后成功刷新的时间. */
  updatedAt: Date | null = null;
  /** 以文件哈希共享的编辑状态. */
  readonly editors = new Map<string, EditorState>();
  /** 是否已经完成首次查询. */
  initialized = false;
  /** PostgREST 客户端. */
  private readonly client = new PostgrestClient();
  /** 当前列表请求的取消控制器. */
  private controller: AbortController | null = null;
  /** 当前列表请求的递增序号. */
  private request = 0;

  /** 启动初始查询和可见页面的定时刷新. */
  start(): void {
    void this.refresh();
    window.setInterval(() => { if (!document.hidden) void this.refresh(); }, 10000);
    document.addEventListener("visibilitychange", () => { if (!document.hidden) void this.refresh(); });
  }

  /** 更新筛选条件并重新查询第一页. */
  setQuery(change: Partial<WatchQuery>): void {
    this.query = { ...this.query, ...change };
    void this.refresh();
  }

  /** 重新查询当前页, 过期结果不会覆盖新结果. */
  async refresh(): Promise<boolean> {
    this.controller?.abort();
    const controller = new AbortController();
    this.controller = controller;
    const request = ++this.request;
    this.loading = true;
    this.notify();
    try {
      const page = await this.client.listWatch({ ...this.query }, controller.signal);
      if (request !== this.request) return false;
      if (this.query.page > 0 && this.query.page * this.query.pageSize >= page.total) {
        this.query.page = Math.max(0, Math.ceil(page.total / this.query.pageSize) - 1);
        return this.refresh();
      }
      this.page = page;
      this.error = null;
      this.initialized = true;
      this.updatedAt = new Date();
      return true;
    } catch (error) {
      if (controller.signal.aborted || request !== this.request) return false;
      this.error = error instanceof Error ? error.message : String(error);
      return false;
    } finally {
      if (request === this.request) {
        this.loading = false;
        this.notify();
      }
    }
  }

  /** 打开某个文件的共享编辑器. */
  edit(hash: string, value: string | null): void {
    if (!this.editors.has(hash)) this.editors.set(hash, { draft: value ?? "", saving: false, error: null });
    this.notify();
  }

  /** 记录输入草稿而不重新绘制输入框. */
  draft(hash: string, value: string): void {
    const state = this.editors.get(hash);
    if (state && !state.saving) state.draft = value;
  }

  /** 取消未提交的编辑. */
  cancel(hash: string): void {
    if (!this.editors.get(hash)?.saving) this.editors.delete(hash);
    this.notify();
  }

  /** 保存文件关联并立即从视图重读全部关联数据. */
  async save(hash: string): Promise<void> {
    const state = this.editors.get(hash);
    if (!state || state.saving) return;
    let value: string | null;
    try {
      value = parseEpisodeId(state.draft);
    } catch (error) {
      state.error = error instanceof Error ? error.message : String(error);
      this.notify();
      return;
    }
    state.saving = true;
    state.error = null;
    this.notify();
    try {
      await this.client.updateEpisodeId(hash, value);
    } catch (error) {
      state.saving = false;
      state.error = error instanceof Error ? error.message : String(error);
      this.notify();
      return;
    }
    const refreshed = await this.refresh();
    this.editors.delete(hash);
    if (!refreshed && this.error) {
      this.error = `修改已提交, 但视图刷新失败: ${this.error}`;
    }
    this.notify();
  }

  /** 通知页面状态发生变化. */
  private notify(): void {
    this.dispatchEvent(new Event("change"));
  }
}
