
-- src/kg/sql/media/select_paths.sql
-- 匹配任一媒体根目录的子路径, 字面匹配不解释通配符.
SELECT absolute_path, file_sha256
FROM public.path_file
WHERE EXISTS (
    SELECT 1
    FROM UNNEST($1::TEXT[]) AS root(prefix)
    WHERE starts_with(absolute_path, root.prefix)
)
