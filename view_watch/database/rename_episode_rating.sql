
-- view_watch/database/rename_episode_rating.sql
-- 在事务中重命名观看评分字段, 视图输出列和评分约束.
BEGIN;
LOCK TABLE public.watch_episode IN ACCESS EXCLUSIVE MODE;
DO $$
BEGIN
    IF NOT EXISTS (
        SELECT 1 FROM pg_attribute
        WHERE attrelid = 'public.watch_episode'::regclass
          AND attname = 'reating' AND attnum > 0 AND NOT attisdropped
    ) OR EXISTS (
        SELECT 1 FROM pg_attribute
        WHERE attrelid = 'public.watch_episode'::regclass
          AND attname = 'rating' AND attnum > 0 AND NOT attisdropped
    ) THEN
        RAISE EXCEPTION 'watch_episode 评分字段不符合迁移前置条件';
    END IF;
    IF NOT EXISTS (
        SELECT 1 FROM pg_constraint
        WHERE conrelid = 'public.watch_episode'::regclass AND conname = 'reating_range'
    ) THEN
        RAISE EXCEPTION '缺少 watch_episode.reating_range 约束';
    END IF;
END
$$;
ALTER TABLE public.watch_episode RENAME COLUMN reating TO rating;
ALTER TABLE public.watch_episode RENAME CONSTRAINT reating_range TO rating_range;
ALTER VIEW public.view_watch_episode RENAME COLUMN reating TO rating;
NOTIFY pgrst, 'reload schema';
COMMIT;
