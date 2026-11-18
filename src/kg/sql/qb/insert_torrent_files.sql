
-- src/kg/sql/qb/insert_torrent_files.sql
-- 批量登记一个种子中的相对路径.
INSERT INTO public.torrent_file (torrent_hash_v1, relative_path)
SELECT $1, relative_path FROM UNNEST($2::TEXT[]) AS input(relative_path)
ON CONFLICT (torrent_hash_v1, relative_path) DO NOTHING
