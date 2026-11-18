
-- src/kg/sql/qb/insert_in_qb.sql
-- 记录当前客户端中的种子.
INSERT INTO public.in_qb (torrent_hash_v1)
VALUES ($1) ON CONFLICT DO NOTHING
