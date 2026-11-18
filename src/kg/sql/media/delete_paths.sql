
-- src/kg/sql/media/delete_paths.sql
-- 删除扫描范围中已不存在的路径, 不删除内容记录.
DELETE FROM public.path_file
WHERE absolute_path = ANY($1::TEXT[])
