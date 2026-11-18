
-- src/kg/sql/qb/upsert_paths.sql
-- 批量更新路径索引, 同批重复路径以最后一条为准.
INSERT INTO public.path_file (absolute_path, file_sha256, created_time, update_time)
SELECT DISTINCT ON (path) path, sha256, created_at, CURRENT_TIMESTAMP
FROM UNNEST($1::TEXT[], $2::BYTEA[], $3::TIMESTAMPTZ[])
WITH ORDINALITY AS input(path, sha256, created_at, ord)
ORDER BY path, ord DESC
ON CONFLICT (absolute_path) DO UPDATE SET
    file_sha256  = EXCLUDED.file_sha256,
    created_time = EXCLUDED.created_time,
    update_time  = EXCLUDED.update_time
