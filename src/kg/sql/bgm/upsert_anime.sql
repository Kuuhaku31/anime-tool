
-- src/kg/sql/bgm/upsert_anime.sql
-- 更新动画字段和最后同步时间.
INSERT INTO public.bangumi_anime
(bgm_ani_id, title, title_cn, airdate, summary, ep_count, object_count, cover_url, official_site, update_at)
VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, CURRENT_TIMESTAMP)
ON CONFLICT (bgm_ani_id) DO UPDATE SET
    title = EXCLUDED.title, title_cn = EXCLUDED.title_cn, airdate = EXCLUDED.airdate,
    summary = EXCLUDED.summary, ep_count = EXCLUDED.ep_count,
    object_count = EXCLUDED.object_count, cover_url = EXCLUDED.cover_url,
    official_site = EXCLUDED.official_site, update_at = CURRENT_TIMESTAMP
