
// view_watch/src/api/watch-episode-client.ts
// 查询观看剧集视图并通过 PostgREST 写入或删除观看记录.
import type { EpisodePage, EpisodeQuery, WatchEpisodeRow } from "../models/watch-episode.ts";
import { responseError } from "./postgrest-client.ts";

/** 保存到数据库的观看记录. */
export interface WatchEpisodeValue {
  /** Bangumi 剧集主键. */ bgm_epi_id: number;
  /** 具有时区的观看时间. */ watched_at: string;
  /** 可为空的评分. */ rating: number | null;
}

/** 获取页面, 写入或删除观看记录. */
export class WatchEpisodeClient {
  /** 查询视图, 请求精确总数并避免 bigint 精度丢失. */
  async list(query: EpisodeQuery, signal: AbortSignal): Promise<EpisodePage> {
    const params = new URLSearchParams();
    params.set("select", "bgm_epi_id,bgm_ani_id,ddp_ani_id::text,mikan_rss_id,mikan_rss_title,bgm_ani_title,bgm_epi_ep,bgm_epi_title,watched_at,rating,bgm_epi_airdate,bgm_epi_weekday");
    const allowed = ["bgm_epi_id", "bgm_ani_id", "bgm_ani_title", "bgm_epi_ep", "bgm_epi_airdate", "watched_at", "rating"];
    if (!allowed.includes(query.sort) || !["asc", "desc"].includes(query.direction)) throw new Error("无效排序.");
    params.set("order", [...new Set([query.sort, "bgm_epi_id", "ddp_ani_id", "mikan_rss_id"])]
      .map((field) => `${field}.${field === query.sort ? query.direction : "asc"}.nullslast`).join(","));
    const search = query.search.trim();
    if (search) {
      const pattern = `"*${search.replace(/\\/g, "\\\\").replace(/"/g, '\\"')}*"`;
      const filters = ["bgm_ani_title", "mikan_rss_title", "bgm_epi_title", "bgm_epi_ep"].map((field) => `${field}.ilike.${pattern}`);
      if (/^\d+$/.test(search) && BigInt(search) <= 9223372036854775807n) {
        const id = BigInt(search).toString();
        filters.push(`ddp_ani_id.eq.${id}`);
        if (BigInt(search) <= 2147483647n) {
          filters.push(`bgm_epi_id.eq.${id}`, `bgm_ani_id.eq.${id}`, `mikan_rss_id.eq.${id}`);
        }
      }
      params.set("or", `(${filters.join(",")})`);
    }
    if (query.watched !== "all") params.set("watched_at", query.watched === "yes" ? "not.is.null" : "is.null");
    if (query.watched === "aired-no") {
      // 使用浏览器本地日期与页面放送状态保持一致, 日期比较会排除未知日期.
      const now = new Date();
      const today = `${now.getFullYear()}-${String(now.getMonth() + 1).padStart(2, "0")}-${String(now.getDate()).padStart(2, "0")}`;
      params.set("bgm_epi_airdate", `lte.${today}`);
    }
    if (query.rating === "null") params.set("rating", "is.null");
    else if (/^[1-5]$/.test(query.rating)) params.set("rating", `eq.${query.rating}`);
    else if (query.rating !== "") throw new Error("无效评分筛选.");
    const start = query.page * query.pageSize;
    const response = await fetch(`/rest/view_watch_episode?${params}`, {
      headers: { Range: `${start}-${start + query.pageSize - 1}`, "Range-Unit": "items", Prefer: "count=exact" },
      signal,
    });
    if (!response.ok && response.status !== 416) throw await responseError(response);
    const match = response.headers.get("content-range")?.match(/\/(\d+)$/);
    if (!match) throw new Error("PostgREST 未返回精确记录总数.");
    if (response.status === 416) return { items: [], total: Number(match[1]) };
    const items: unknown = await response.json();
    if (!Array.isArray(items)) throw new Error("PostgREST 返回的剧集数据不是数组.");
    return { items: items as WatchEpisodeRow[], total: Number(match[1]) };
  }

  /** 写入一条或多条记录, 返回实际写入的剧集 ID. */
  async save(values: WatchEpisodeValue[], ignoreDuplicates: boolean): Promise<number[]> {
    const response = await fetch("/rest/watch_episode?on_conflict=bgm_epi_id&select=bgm_epi_id", {
      method: "POST",
      headers: { "Content-Type": "application/json", Prefer: `resolution=${ignoreDuplicates ? "ignore" : "merge"}-duplicates,return=representation` },
      body: JSON.stringify(values),
    });
    return this.ids(response);
  }

  /** 删除指定主键的记录, 返回实际删除的剧集 ID. */
  async delete(ids: number[]): Promise<number[]> {
    if (!ids.length || ids.some((id) => !Number.isInteger(id) || id < 0 || id > 2147483647)) {
      throw new Error("剧集 ID 无效.");
    }
    const params = new URLSearchParams({ bgm_epi_id: `in.(${ids.join(",")})`, select: "bgm_epi_id" });
    const response = await fetch(`/rest/watch_episode?${params}`, {
      method: "DELETE", headers: { Prefer: "return=representation" },
    });
    return this.ids(response);
  }

  /** 校验写入响应并提取实际操作的主键. */
  private async ids(response: Response): Promise<number[]> {
    if (!response.ok) throw await responseError(response);
    const rows: unknown = await response.json();
    if (!Array.isArray(rows) || !rows.every((row: unknown) =>
      typeof row === "object" && row !== null && "bgm_epi_id" in row
      && typeof row.bgm_epi_id === "number" && Number.isInteger(row.bgm_epi_id))) {
      throw new Error("PostgREST 未返回有效的操作结果.");
    }
    return rows.map((row) => row.bgm_epi_id as number);
  }
}
