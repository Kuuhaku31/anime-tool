
-- src/kg/sql/mikan/select_ids.sql
-- 读取非空且去重后的蜜柑 RSS ID.
SELECT DISTINCT mikan_rss_id FROM public.ids
WHERE mikan_rss_id IS NOT NULL ORDER BY mikan_rss_id
