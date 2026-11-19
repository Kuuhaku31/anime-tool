
-- src/kg/sql/media/select_unidentified.sql
-- 查找尚未识别且拥有路径索引的文件, 每个哈希按路径排序只取第一条.
SELECT m.sha256, f.byte_count, f.pre16mib_md5, p.absolute_path
FROM public.media_file AS m
JOIN public.file AS f ON f.sha256 = m.sha256
JOIN LATERAL (
    SELECT absolute_path
    FROM public.path_file
    WHERE file_sha256 = m.sha256
    ORDER BY absolute_path COLLATE "C"
    LIMIT 1
) AS p ON true
WHERE m.ddp_epi_id IS NULL
ORDER BY m.sha256
