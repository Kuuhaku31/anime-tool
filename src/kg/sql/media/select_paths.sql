
-- src/kg/sql/media/select_paths.sql
-- 查找本次扫描到的精确文件路径.
SELECT absolute_path, file_sha256
FROM public.path_file
WHERE absolute_path = ANY($1::TEXT[])
