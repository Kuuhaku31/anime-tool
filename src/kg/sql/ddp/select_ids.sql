
-- src/kg/sql/ddp/select_ids.sql
-- 读取非空且去重后的 Dandanplay 动画 ID.
SELECT DISTINCT ddp_ani_id FROM public.ids
WHERE ddp_ani_id IS NOT NULL ORDER BY ddp_ani_id
