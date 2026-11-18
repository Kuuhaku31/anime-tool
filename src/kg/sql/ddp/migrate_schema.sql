
-- src/kg/sql/ddp/migrate_schema.sql
-- 创建 Dandanplay 表并扩大 ID 类型, 事务失败时整体回滚.
BEGIN;
LOCK TABLE public.ids, public.media_file IN ACCESS EXCLUSIVE MODE;

DO $precondition$
BEGIN
    IF current_database() <> 'main' THEN
        RAISE EXCEPTION 'Dandanplay 结构迁移必须在 main 数据库执行';
    END IF;
    IF to_regclass('public.dandanplay_anime') IS NOT NULL
        OR to_regclass('public.dandanplay_episode') IS NOT NULL THEN
        RAISE EXCEPTION 'Dandanplay 表已存在, 不重复执行迁移';
    END IF;
    IF to_regclass('public.view_meida_file') IS NULL THEN
        RAISE EXCEPTION '缺少预期的 public.view_meida_file';
    END IF;
    IF EXISTS (SELECT 1 FROM public.media_file WHERE ddp_epi_id IS NOT NULL) THEN
        RAISE EXCEPTION 'media_file.ddp_epi_id 非空, 需先映射现有剧集再迁移';
    END IF;
END
$precondition$;

DROP VIEW public.view_meida_file;

ALTER TABLE public.ids
    ALTER COLUMN ddp_ani_id TYPE bigint USING ddp_ani_id::bigint;
ALTER TABLE public.media_file
    ALTER COLUMN ddp_epi_id TYPE bigint USING ddp_epi_id::bigint;

CREATE TABLE public.dandanplay_anime (
    ddp_ani_id bigint PRIMARY KEY,
    bgm_ani_id bigint,
    title text,
    intro text,
    summary text,
    episode_count integer,
    image_url text,
    update_at timestamp with time zone NOT NULL DEFAULT CURRENT_TIMESTAMP,
    CONSTRAINT dandanplay_anime_id_nonnegative CHECK (ddp_ani_id >= 0),
    CONSTRAINT dandanplay_anime_bgm_ani_id_nonnegative
        CHECK (bgm_ani_id IS NULL OR bgm_ani_id >= 0),
    CONSTRAINT dandanplay_anime_episode_count_nonnegative
        CHECK (episode_count IS NULL OR episode_count >= 0)
);

CREATE TABLE public.dandanplay_episode (
    ddp_epi_id bigint PRIMARY KEY,
    ddp_ani_id bigint NOT NULL,
    title text,
    ep text,
    airdate timestamp without time zone,
    update_at timestamp with time zone NOT NULL DEFAULT CURRENT_TIMESTAMP,
    CONSTRAINT dandanplay_episode_id_nonnegative CHECK (ddp_epi_id >= 0),
    CONSTRAINT dandanplay_episode_anime_fk
        FOREIGN KEY (ddp_ani_id)
        REFERENCES public.dandanplay_anime (ddp_ani_id)
        ON DELETE CASCADE
);

CREATE INDEX dandanplay_episode_lookup_idx
    ON public.dandanplay_episode (ddp_ani_id, ep);

ALTER TABLE public.media_file
    ADD CONSTRAINT dandanplay_media_episode_fk
    FOREIGN KEY (ddp_epi_id)
    REFERENCES public.dandanplay_episode (ddp_epi_id)
    ON UPDATE NO ACTION
    ON DELETE NO ACTION;

CREATE VIEW public.view_meida_file AS
SELECT encode(m.sha256, 'hex') AS file_sha256,
    encode(f.pre16mib_md5, 'hex') AS pre16mib_md5,
    f.byte_count AS size_int,
    format_bytes(f.byte_count) AS size_str,
    p.absolute_path AS real_file,
    p.created_time AS real_file_create_at,
    p.update_time AS real_file_update_at,
    m.ddp_epi_id,
    m.postiton AS watch_postiton,
    m.video_stream_index AS watch_video_stream_index,
    m.audio_stream_index AS watch_audio_stream_index,
    m.subtitle_stream_index,
    encode(tf.torrent_hash_v1, 'hex') AS torrent_hash_v1,
    t.category AS torrent_category,
    t.title AS torrent_title,
    tf.relative_path AS torrent_relative_path,
    COALESCE(tf.torrent_file_count, 0::bigint) AS torrent_file_count,
    mikan_rss.rss_id AS mikan_rss_id,
    mikan_rss.title AS mikan_rss_title,
    mikan_torrent.title AS mikan_torrent_title,
    mikan_torrent.description AS mikan_torrent_description,
    mikan_torrent.pub_datetime AS mikan_torrent_pub_at,
    mikan_rss.update_datetime AS mikan_rss_update_at,
    mikan_torrent.update_time AS mikan_torrent_update_at
FROM media_file m
LEFT JOIN path_file p ON p.file_sha256 = m.sha256
LEFT JOIN file f ON f.sha256 = p.file_sha256
LEFT JOIN LATERAL (
    SELECT tf_1.torrent_hash_v1, tf_1.relative_path, tf_1.sha256,
        count(*) OVER () AS torrent_file_count
    FROM torrent_file tf_1
    WHERE tf_1.sha256 = m.sha256
    ORDER BY tf_1.torrent_hash_v1, tf_1.relative_path
    LIMIT 1
) tf ON true
LEFT JOIN torrent t ON t.hash_v1 = tf.torrent_hash_v1
LEFT JOIN mikan_torrent ON t.hash_v1 = mikan_torrent.hash_v1
LEFT JOIN mikan_rss ON mikan_torrent.mikan_rss_id = mikan_rss.rss_id;

ALTER VIEW public.view_meida_file OWNER TO kuuhaku;

COMMIT;
