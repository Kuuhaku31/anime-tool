
-- src/kg/sql/ddp/upsert_episode.sql
-- 更新 Dandanplay 剧集字段和最后同步时间.
INSERT INTO public.dandanplay_episode
(ddp_epi_id, ddp_ani_id, title, ep, airdate, update_at)
VALUES ($1, $2, $3, $4, $5, CURRENT_TIMESTAMP)
ON CONFLICT (ddp_epi_id) DO UPDATE SET
    ddp_ani_id = EXCLUDED.ddp_ani_id,
    title = EXCLUDED.title,
    ep = EXCLUDED.ep,
    airdate = EXCLUDED.airdate,
    update_at = CURRENT_TIMESTAMP
