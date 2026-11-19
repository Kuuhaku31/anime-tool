
-- src/kg/sql/ddp/upsert_danmaku.sql
-- 更新弹幕内容和解析字段, 不删除 API 本次未返回的历史弹幕.
INSERT INTO public.dandanplay_danmaku (
    ddp_epi_id, cid, appear_time, mode, color, user_id, content, raw_p, update_at
)
VALUES ($1, $2, $3, $4, $5, $6, $7, $8, CURRENT_TIMESTAMP)
ON CONFLICT (ddp_epi_id, cid) DO UPDATE SET
    appear_time = EXCLUDED.appear_time,
    mode = EXCLUDED.mode,
    color = EXCLUDED.color,
    user_id = EXCLUDED.user_id,
    content = EXCLUDED.content,
    raw_p = EXCLUDED.raw_p,
    update_at = CURRENT_TIMESTAMP
