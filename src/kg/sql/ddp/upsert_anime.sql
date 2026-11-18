
-- src/kg/sql/ddp/upsert_anime.sql
-- 更新 Dandanplay 动画字段和最后同步时间.
INSERT INTO public.dandanplay_anime
(ddp_ani_id, bgm_ani_id, title, intro, summary, episode_count, image_url, update_at)
VALUES ($1, $2, $3, $4, $5, $6, $7, CURRENT_TIMESTAMP)
ON CONFLICT (ddp_ani_id) DO UPDATE SET
    bgm_ani_id = EXCLUDED.bgm_ani_id,
    title = EXCLUDED.title,
    intro = EXCLUDED.intro,
    summary = EXCLUDED.summary,
    episode_count = EXCLUDED.episode_count,
    image_url = EXCLUDED.image_url,
    update_at = CURRENT_TIMESTAMP
