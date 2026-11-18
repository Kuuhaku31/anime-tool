
-- src/kg/sql/qb/update_torrent_file_hashes.sql
-- 批量关联种子文件与其哈希, 并返回首个未命中的路径.
WITH input AS (
    SELECT hash, path, sha256, ord
    FROM UNNEST($1::BYTEA[], $2::TEXT[], $3::BYTEA[])
    WITH ORDINALITY AS data(hash, path, sha256, ord)
), updated AS (
    UPDATE public.torrent_file AS tf SET sha256 = latest.sha256
    FROM (
        SELECT DISTINCT ON (hash, path) hash, path, sha256
        FROM input ORDER BY hash, path, ord DESC
    ) AS latest
    WHERE tf.torrent_hash_v1 = latest.hash AND tf.relative_path = latest.path
    RETURNING tf.torrent_hash_v1, tf.relative_path
)
SELECT input.path FROM input
LEFT JOIN updated ON updated.torrent_hash_v1 = input.hash AND updated.relative_path = input.path
WHERE updated.torrent_hash_v1 IS NULL ORDER BY input.ord LIMIT 1
