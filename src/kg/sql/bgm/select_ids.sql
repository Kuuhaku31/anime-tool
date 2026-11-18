
-- src/kg/sql/bgm/select_ids.sql
-- 读取非空且去重后的 Bangumi 动画 ID.
SELECT DISTINCT bgm_ani_id FROM public.ids
WHERE bgm_ani_id IS NOT NULL ORDER BY bgm_ani_id
