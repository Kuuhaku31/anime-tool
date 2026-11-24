
-- src/kg/sql/mikan/select_ids.sql
-- 从关联视图读取非空且去重后的蜜柑 RSS ID.
SELECT DISTINCT mikan_rss_id FROM public.view_ids
WHERE mikan_rss_id IS NOT NULL ORDER BY mikan_rss_id
