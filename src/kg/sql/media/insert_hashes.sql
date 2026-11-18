
-- src/kg/sql/media/insert_hashes.sql
-- 登记内容哈希, 保留已登记媒体的关联和流索引字段.
INSERT INTO public.media_file (sha256)
SELECT DISTINCT sha256 FROM UNNEST($1::BYTEA[]) AS input(sha256)
ON CONFLICT (sha256) DO NOTHING
