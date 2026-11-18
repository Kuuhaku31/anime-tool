
-- src/kg/sql/qb/upsert_torrent.sql
-- 写入或更新种子元数据和原始内容.
INSERT INTO public.torrent (hash_v1, title, category, contents)
VALUES ($1, $2, $3, $4)
ON CONFLICT (hash_v1) DO UPDATE SET
    title = EXCLUDED.title,
    category = EXCLUDED.category,
    contents = EXCLUDED.contents
