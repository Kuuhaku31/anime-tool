
-- src/kg/sql/bgm/upsert_episode.sql
-- 更新剧集字段和最后同步时间.
INSERT INTO public.bangumi_episode
(bgm_epi_id, bgm_ani_id, title, title_cn, airdate, summary, ep, sort, duration, update_at)
VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, CURRENT_TIMESTAMP)
ON CONFLICT (bgm_epi_id) DO UPDATE SET
    bgm_ani_id = EXCLUDED.bgm_ani_id, title = EXCLUDED.title,
    title_cn = EXCLUDED.title_cn, airdate = EXCLUDED.airdate, summary = EXCLUDED.summary,
    ep = EXCLUDED.ep, sort = EXCLUDED.sort, duration = EXCLUDED.duration,
    update_at = CURRENT_TIMESTAMP
