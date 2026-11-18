
-- src/kg/sql/qb/select_path_hashes.sql
-- 按输入顺序查找路径对应的文件哈希, 包括未命中的路径.
SELECT pf.file_sha256
FROM UNNEST($1::TEXT[]) WITH ORDINALITY AS input(absolute_path, ord)
LEFT JOIN public.path_file AS pf ON pf.absolute_path = input.absolute_path
ORDER BY input.ord
