
-- src/kg/sql/qb/upsert_files.sql
-- 批量更新文件索引, 同批重复哈希以最后一条为准.
INSERT INTO public.file (sha256, pre16mib_md5, byte_count)
SELECT DISTINCT ON (sha256) sha256, md5, size
FROM UNNEST($1::BYTEA[], $2::BYTEA[], $3::BIGINT[])
WITH ORDINALITY AS input(sha256, md5, size, ord)
ORDER BY sha256, ord DESC
ON CONFLICT (sha256) DO UPDATE SET
    pre16mib_md5 = EXCLUDED.pre16mib_md5,
    byte_count   = EXCLUDED.byte_count
