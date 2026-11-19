
-- src/kg/sql/media/select_all_paths.sql
-- 读取 path_file 表中全部绝对路径.
SELECT absolute_path
FROM public.path_file
ORDER BY absolute_path
