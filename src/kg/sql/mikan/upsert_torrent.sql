
-- src/kg/sql/mikan/upsert_torrent.sql
-- 更新 RSS 中的种子及其元数据.
INSERT INTO public.mikan_torrent
(hash_v1, mikan_rss_id, file_byte_count, title, description, pub_datetime, update_time)
VALUES ($1, $2, $3, $4, $5, $6, CURRENT_TIMESTAMP)
ON CONFLICT (hash_v1) DO UPDATE SET
    mikan_rss_id    = EXCLUDED.mikan_rss_id,
    file_byte_count = EXCLUDED.file_byte_count,
    title           = EXCLUDED.title,
    description     = EXCLUDED.description,
    pub_datetime    = EXCLUDED.pub_datetime,
    update_time     = EXCLUDED.update_time
