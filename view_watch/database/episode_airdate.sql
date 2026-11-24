
-- view_watch/database/episode_airdate.sql
-- 为观看剧集视图追加放送日期, ISO 星期和蜜柑标题, 保留现有列和授权.
BEGIN;
SET LOCAL lock_timeout = '10s';
CREATE OR REPLACE VIEW public.view_watch_episode AS
SELECT be.bgm_epi_id,
    be.bgm_ani_id,
    da.ddp_ani_id,
    mr.rss_id AS mikan_rss_id,
    ba.title AS bgm_ani_title,
    be.ep AS bgm_epi_ep,
    be.title AS bgm_epi_title,
    we.watched_at,
    we.rating,
    be.airdate AS bgm_epi_airdate,
    EXTRACT(ISODOW FROM be.airdate)::integer AS bgm_epi_weekday,
    mr.title AS mikan_rss_title
FROM public.bangumi_episode be
JOIN public.bangumi_anime ba ON ba.bgm_ani_id = be.bgm_ani_id
LEFT JOIN public.watch_episode we ON be.bgm_epi_id = we.bgm_epi_id
LEFT JOIN public.dandanplay_anime da ON be.bgm_ani_id = da.bgm_ani_id
LEFT JOIN public.mikan_rss mr ON be.bgm_ani_id = mr.bgm_ani_id
WHERE be.bgm_ani_id IN (SELECT ids.bgm_ani_id FROM public.ids);
NOTIFY pgrst, 'reload schema';
COMMIT;
