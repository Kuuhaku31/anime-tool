
-- view_watch/database/grants.sql
-- 创建最小权限角色并授予视图查询及观看记录编辑权限.
DO $$
BEGIN
    IF NOT EXISTS (SELECT 1 FROM pg_roles WHERE rolname = 'watch_user') THEN
        CREATE ROLE watch_user NOLOGIN;
    END IF;
    IF NOT EXISTS (SELECT 1 FROM pg_roles WHERE rolname = 'watch_gateway') THEN
        CREATE ROLE watch_gateway LOGIN;
    END IF;
END
$$;

GRANT watch_user TO watch_gateway;
GRANT USAGE ON SCHEMA public TO watch_user;
GRANT SELECT ON public.view_watch TO watch_user;
GRANT SELECT ON public.view_watch_episode TO watch_user;
GRANT SELECT (sha256, ddp_epi_id) ON public.media_file TO watch_user;
GRANT UPDATE (ddp_epi_id) ON public.media_file TO watch_user;
GRANT SELECT (bgm_epi_id, watched_at, rating) ON public.watch_episode TO watch_user;
GRANT INSERT (bgm_epi_id, watched_at, rating) ON public.watch_episode TO watch_user;
-- PostgREST 的冲突更新会将插入的主键列也列入 UPDATE 字段.
GRANT UPDATE (bgm_epi_id, watched_at, rating) ON public.watch_episode TO watch_user;
GRANT DELETE ON public.watch_episode TO watch_user;
NOTIFY pgrst, 'reload schema';
