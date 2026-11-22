
-- view_watch/database/grants.sql
-- 创建最小权限角色并授予现有视图查询及单列更新权限.
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
GRANT SELECT (sha256, ddp_epi_id) ON public.media_file TO watch_user;
GRANT UPDATE (ddp_epi_id) ON public.media_file TO watch_user;
