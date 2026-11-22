
// view_watch/src/utils/query.ts
// 将筛选条件安全转换为 PostgREST 查询.
import type { WatchQuery } from "../models/watch.ts";

/** 生成视图查询参数, 显式将 bigint 转为文本避免浏览器舍入. */
export function watchParams(query: WatchQuery): URLSearchParams {
  const params = new URLSearchParams();
  params.set("select", [
    "mikan_torrent_hash_v1", "file_sha256", "mikan_rss_id", "mikan_rss_title",
    "mikan_torrent_file_size::text", "file_size", "mikan_torrent_title", "mikan_torrent_description",
    "mikan_torrent_download_url", "mikan_torrent_page_url", "mikan_torrent_pub_at",
    "mikan_torrent_update_at", "in_qb", "file_absolute_path", "file_directory", "file_filename",
    "file_extension", "file_url", "ddp_epi_id::text", "ddp_ani_id::text", "bgm_ani_id",
    "dandanplay_anime_title", "dandanplay_episode_ep", "dandanplay_episode_title",
    "dandanplay_episode_danmaku_update_at", "bangumi_anime_title",
  ].join(","));
  const allowed = new Set(["mikan_rss_id", "mikan_torrent_pub_at", "mikan_torrent_update_at",
    "file_filename", "mikan_torrent_file_size", "ddp_epi_id", "dandanplay_episode_danmaku_update_at"]);
  if (!allowed.has(query.sort) || !["asc", "desc"].includes(query.direction)) throw new Error("无效排序.");
  const order = [query.sort, "mikan_rss_id", "mikan_torrent_title", "file_filename", "file_sha256",
    "file_absolute_path", "mikan_torrent_hash_v1"];
  params.set("order", [...new Set(order)].map((field) =>
    `${field}.${field === query.sort ? query.direction : "asc"}.nullslast`).join(","));
  const search = query.search.trim();
  if (search) {
    const pattern = `"*${search.replace(/\\/g, "\\\\").replace(/"/g, '\\"')}*"`;
    const columns = ["file_filename", "file_absolute_path", "mikan_rss_title", "mikan_torrent_title",
      "dandanplay_anime_title", "dandanplay_episode_title"];
    params.set("or", `(${columns.map((column) => `${column}.ilike.${pattern}`).join(",")})`);
  }
  if (query.ddp === "null") params.set("ddp_epi_id", "is.null");
  if (query.ddp === "unmatchable") params.set("ddp_epi_id", "eq.-1");
  if (query.ddp === "assigned") params.set("ddp_epi_id", "not.is.null");
  if (query.qb !== "all") params.set("in_qb", `eq.${query.qb === "yes" ? 1 : 0}`);
  if (query.editable !== "all") params.set("file_sha256", query.editable === "yes" ? "not.is.null" : "is.null");
  return params;
}

/** 生成 PostgreSQL bytea 主键筛选参数. */
export function hashParams(hash: string): URLSearchParams {
  if (!/^[0-9a-f]{64}$/i.test(hash)) throw new Error("文件 SHA-256 无效.");
  return new URLSearchParams({ sha256: `eq.\\x${hash.toLowerCase()}`, select: "sha256" });
}
