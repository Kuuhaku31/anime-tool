
// view_watch/src/api/postgrest-client.ts
// 通过同源 PostgREST 查询视图并更新文件剧集关联.
import type { WatchPage, WatchQuery, WatchRow } from "../models/watch.ts";
import { hashParams, watchParams } from "../utils/query.ts";

/** 访问同源 PostgREST 的客户端. */
export class PostgrestClient {
  /** 查询视图原始行和精确总数. */
  async listWatch(query: WatchQuery, signal: AbortSignal): Promise<WatchPage> {
    const start = query.page * query.pageSize;
    const response = await fetch(`/rest/view_watch?${watchParams(query)}`, {
      headers: { Range: `${start}-${start + query.pageSize - 1}`, "Range-Unit": "items", Prefer: "count=exact" },
      signal,
    });
    if (!response.ok && response.status !== 416) throw await responseError(response);
    const contentRange = response.headers.get("content-range");
    const match = contentRange?.match(/\/(\d+)$/);
    if (!match) throw new Error("PostgREST 未返回精确记录总数.");
    if (response.status === 416) return { items: [], total: Number(match[1]) };
    const items: unknown = await response.json();
    if (!Array.isArray(items)) throw new Error("PostgREST 返回的视图数据不是数组.");
    return { items: items as WatchRow[], total: Number(match[1]) };
  }

  /** 按 SHA-256 更新唯一文件, 并确认数据库确实匹配一行. */
  async updateEpisodeId(hash: string, value: string | null): Promise<void> {
    // ID 已校验为整数, 原样编码为 JSON 数字可避免 JavaScript 浮点数舍入.
    const body = `{"ddp_epi_id":${value === null ? "null" : value}}`;
    const response = await fetch(`/rest/media_file?${hashParams(hash)}`, {
      method: "PATCH",
      headers: { "Content-Type": "application/json", Prefer: "return=representation" },
      body,
    });
    if (!response.ok) throw await responseError(response);
    const rows: unknown = await response.json();
    if (!Array.isArray(rows) || rows.length !== 1) throw new Error("文件不存在或无法更新, 请刷新后重试.");
  }
}

/** 从 PostgREST 响应中提取安全可读的错误. */
async function responseError(response: Response): Promise<Error> {
  const text = await response.text();
  try {
    const body: unknown = JSON.parse(text);
    if (body && typeof body === "object" && "message" in body && typeof body.message === "string") {
      const details = "details" in body && typeof body.details === "string" ? `: ${body.details}` : "";
      return new Error(`HTTP ${response.status}: ${body.message}${details}`);
    }
  } catch {
    // 非 JSON 错误仍通过状态码明确报告.
  }
  return new Error(`HTTP ${response.status}: ${response.statusText || "服务请求失败"}`);
}
