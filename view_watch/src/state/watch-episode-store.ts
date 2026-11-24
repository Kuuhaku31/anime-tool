
// view_watch/src/state/watch-episode-store.ts
// 管理剧集查询, 行内草稿和跨页批量操作.
import { WatchEpisodeClient, type WatchEpisodeValue } from "../api/watch-episode-client.ts";
import type { EpisodeDraft, EpisodePage, EpisodeQuery, WatchEpisodeRow } from "../models/watch-episode.ts";

/** 转换数据库时间为本地日期时间输入值. */
export function localDateTime(value: string | null): string {
  if (!value) return "";
  const date = new Date(value);
  if (!Number.isFinite(date.getTime())) throw new Error("数据库返回了无效观看时间.");
  const shifted = new Date(date.getTime() - date.getTimezoneOffset() * 60000);
  return shifted.toISOString().slice(0, 19);
}

/** 管理剧集页面数据和独立事务操作. */
export class WatchEpisodeStore extends EventTarget {
  /** 当前筛选和分页. */
  query: EpisodeQuery = { search: "", watched: "all", rating: "", sort: "bgm_epi_id", direction: "asc", page: 0, pageSize: 100 };
  /** 当前页和总数. */
  page: EpisodePage = { items: [], total: 0 };
  /** 上次成功查询时间. */
  updatedAt: Date | null = null;
  /** 全局错误信息. */
  error: string | null = null;
  /** 全局操作反馈. */
  message = "";
  /** 是否正在加载. */
  loading = false;
  /** 是否完成首次查询. */
  initialized = false;
  /** 是否正在批量操作. */
  bulkSaving = false;
  /** 是否启用多选. */
  selectionEnabled = false;
  /** 跨页选择和最近一次看到的观看状态. */
  readonly selected = new Map<number, boolean>();
  /** 每个剧集的编辑草稿. */
  readonly drafts = new Map<number, EpisodeDraft>();
  /** PostgREST 客户端. */
  private readonly client = new WatchEpisodeClient();
  /** 当前查询取消控制器. */
  private controller: AbortController | null = null;
  /** 查询序号, 防止旧响应覆盖新响应. */
  private request = 0;

  /** 按新条件重新加载. */
  setQuery(change: Partial<EpisodeQuery>): void {
    this.query = { ...this.query, ...change };
    void this.refresh();
  }

  /** 加载当前页并修正越界页码. */
  async refresh(): Promise<boolean> {
    this.controller?.abort();
    const controller = new AbortController();
    this.controller = controller;
    const request = ++this.request;
    this.loading = true;
    this.notify();
    try {
      const page = await this.client.list({ ...this.query }, controller.signal);
      if (request !== this.request) return false;
      if (this.query.page > 0 && this.query.page * this.query.pageSize >= page.total) {
        this.query.page = Math.max(0, Math.ceil(page.total / this.query.pageSize) - 1);
        return this.refresh();
      }
      this.page = page;
      for (const row of page.items) {
        if (row.bgm_epi_id !== null && this.selected.has(row.bgm_epi_id)) {
          this.selected.set(row.bgm_epi_id, row.watched_at !== null);
        }
      }
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

  /** 返回已有草稿或根据视图行创建新草稿. */
  draft(row: WatchEpisodeRow): EpisodeDraft {
    if (row.bgm_epi_id === null) throw new Error("剧集 ID 为空, 无法编辑.");
    let draft = this.drafts.get(row.bgm_epi_id);
    if (!draft) {
      draft = { originalTime: row.watched_at, time: localDateTime(row.watched_at),
        rating: row.rating === null ? "" : String(row.rating), timeChanged: false, dirty: false, saving: false, error: null };
      this.drafts.set(row.bgm_epi_id, draft);
    }
    return draft;
  }

  /** 丢弃当前剧集未保存的草稿. */
  cancel(id: number): void {
    if (!this.drafts.get(id)?.saving) this.drafts.delete(id);
    this.notify();
  }

  /** 保存草稿, 新记录只填写评分时补充当前时间. */
  async save(row: WatchEpisodeRow): Promise<void> {
    const id = row.bgm_epi_id;
    if (id === null || this.bulkSaving) return;
    const draft = this.draft(row);
    if (draft.saving) return;
    try {
      if (draft.rating !== "" && !/^[1-5]$/.test(draft.rating)) throw new Error("评分必须为空或 1 到 5 的整数.");
      if (!draft.originalTime && !draft.time && !draft.rating) throw new Error("请填写观看时间或评分.");
      if (draft.originalTime && !draft.time) throw new Error("已有观看记录的时间不能为空.");
      let time = draft.originalTime;
      if (draft.timeChanged || !time) {
        if (draft.time) {
          const date = new Date(draft.time);
          const normalized = draft.time.length === 16 ? `${draft.time}:00` : draft.time;
          if (!Number.isFinite(date.getTime()) || localDateTime(date.toISOString()) !== normalized) {
            throw new Error("观看时间无效或不存在, 请重新输入.");
          }
          time = date.toISOString();
        } else {
          time = new Date().toISOString();
        }
      }
      await this.writeOne(id, draft, { bgm_epi_id: id, watched_at: time, rating: draft.rating ? Number(draft.rating) : null }, false);
    } catch (error) {
      draft.error = error instanceof Error ? error.message : String(error);
      this.notify();
    }
  }

  /** 为未观看剧集创建当前时间和空评分, 不覆盖并发创建的记录. */
  async markWatched(row: WatchEpisodeRow): Promise<void> {
    const id = row.bgm_epi_id;
    if (id === null || row.watched_at !== null || this.bulkSaving) return;
    const draft = this.draft(row);
    if (draft.saving) return;
    if (draft.dirty) {
      draft.error = "请先保存或取消未提交的编辑.";
      this.notify();
      return;
    }
    await this.writeOne(id, draft, { bgm_epi_id: id, watched_at: new Date().toISOString(), rating: null }, true);
  }

  /** 确认并删除已有观看记录. */
  async deleteRow(row: WatchEpisodeRow): Promise<void> {
    const id = row.bgm_epi_id;
    if (id === null || row.watched_at === null || this.bulkSaving || !window.confirm(`删除剧集 ${id} 的观看记录?`)) return;
    const draft = this.draft(row);
    if (draft.saving) return;
    draft.saving = true;
    draft.error = null;
    this.notify();
    try {
      const ids = await this.client.delete([id]);
      if (ids.length !== 1 || ids[0] !== id) throw new Error("记录已不存在或无法删除, 请刷新后重试.");
      this.drafts.delete(id);
      this.selected.delete(id);
      this.message = "观看记录已删除.";
      if (!await this.refresh()) this.error = `删除已提交, 但视图刷新失败: ${this.error ?? "请求已取消"}`;
    } catch (error) {
      draft.error = error instanceof Error ? error.message : String(error);
    } finally {
      draft.saving = false;
      this.notify();
    }
  }

  /** 切换多选模式并在关闭时清除选择. */
  setSelectionEnabled(enabled: boolean): void {
    this.selectionEnabled = enabled;
    if (!enabled) this.selected.clear();
    this.message = "";
    this.notify();
  }

  /** 按剧集主键选择或取消选择, 同剧集重复行共享状态. */
  select(row: WatchEpisodeRow, checked: boolean): void {
    if (row.bgm_epi_id === null) return;
    if (checked) this.selected.set(row.bgm_epi_id, row.watched_at !== null);
    else this.selected.delete(row.bgm_epi_id);
    this.notify();
  }

  /** 批量创建缺失记录或确认后删除已有记录. */
  async bulk(remove: boolean): Promise<void> {
    if (this.bulkSaving || [...this.drafts.values()].some((draft) => draft.saving)) return;
    const ids = [...this.selected].filter(([, watched]) => watched === remove).map(([id]) => id);
    if (!ids.length) return;
    if (ids.some((id) => this.drafts.get(id)?.dirty)) {
      this.error = "请先保存或取消所选剧集未提交的编辑.";
      this.notify();
      return;
    }
    if (remove && !window.confirm(`确认删除选中的 ${ids.length} 条观看记录?`)) return;
    this.bulkSaving = true;
    this.message = "";
    this.error = null;
    this.notify();
    try {
      const now = new Date().toISOString();
      const changed = remove ? await this.client.delete(ids)
        : await this.client.save(ids.map((id) => ({ bgm_epi_id: id, watched_at: now, rating: null })), true);
      const expected = new Set(ids);
      if (new Set(changed).size !== changed.length || changed.some((id) => !expected.has(id))) {
        throw new Error("服务器返回了不属于选择范围的操作结果, 请刷新后核对.");
      }
      for (const id of changed) {
        this.selected.delete(id);
        this.drafts.delete(id);
      }
      this.message = `成功${remove ? "删除" : "创建"} ${changed.length} 条观看记录.`
        + (changed.length !== ids.length ? ` ${ids.length - changed.length} 条已发生并发变化, 请刷新后核对.` : "");
      if (!await this.refresh()) this.error = `批量操作已提交, 但视图刷新失败: ${this.error ?? "请求已取消"}`;
    } catch (error) {
      this.error = error instanceof Error ? error.message : String(error);
    } finally {
      this.bulkSaving = false;
      this.notify();
    }
  }

  /** 执行单行写入并反馈并发冲突. */
  private async writeOne(id: number, draft: EpisodeDraft, value: WatchEpisodeValue, ignore: boolean): Promise<void> {
    draft.saving = true;
    draft.error = null;
    this.notify();
    try {
      const ids = await this.client.save([value], ignore);
      if (ids.length !== 1 || ids[0] !== id) {
        throw new Error("记录已被其他用户创建或无法保存, 请刷新后重试.");
      }
      this.drafts.delete(id);
      this.selected.delete(id);
      this.message = "观看记录已保存.";
      if (!await this.refresh()) this.error = `修改已提交, 但视图刷新失败: ${this.error ?? "请求已取消"}`;
    } catch (error) {
      draft.error = error instanceof Error ? error.message : String(error);
    } finally {
      draft.saving = false;
      this.notify();
    }
  }

  /** 通知页面状态变化. */
  private notify(): void {
    this.dispatchEvent(new Event("change"));
  }
}
