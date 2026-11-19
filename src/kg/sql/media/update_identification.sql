
-- src/kg/sql/media/update_identification.sql
-- 仅写入尚未识别的记录, 不覆盖并发写入的匹配结果.
UPDATE public.media_file
SET ddp_epi_id = $2
WHERE sha256 = $1 AND ddp_epi_id IS NULL
