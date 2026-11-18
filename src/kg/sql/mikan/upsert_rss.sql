
-- src/kg/sql/mikan/upsert_rss.sql
-- 更新 RSS 标题和最后同步时间.
INSERT INTO public.mikan_rss (rss_id, title, update_datetime)
VALUES ($1, $2, CURRENT_TIMESTAMP)
ON CONFLICT (rss_id) DO UPDATE SET title = EXCLUDED.title, update_datetime = EXCLUDED.update_datetime
