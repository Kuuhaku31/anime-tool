# Dandanplay 动画和剧集表结构

`public.dandanplay_anime` 保存 Dandanplay 动画信息,
`public.dandanplay_episode` 保存对应的剧集. 数据来自
`GET /api/v2/bangumi/{bangumiId}`.

## public.dandanplay_anime

| 字段            | PostgreSQL 类型 | NULL | 默认值              | API 来源或说明                                            |
| --------------- | --------------- | ---- | ------------------- | --------------------------------------------------------- |
| `ddp_ani_id`    | `bigint`        | 否   | 无                  | 主键, `>= 0`; `bangumi.animeId`                           |
| `bgm_ani_id`    | `bigint`        | 是   | NULL                | 从 `bangumi.bangumiUrl` 的 `/subject/{id}` 提取, 不设外键 |
| `title`         | `text`          | 是   | NULL                | `bangumi.animeTitle`                                      |
| `intro`         | `text`          | 是   | NULL                | `bangumi.intro`                                           |
| `summary`       | `text`          | 是   | NULL                | `bangumi.summary`                                         |
| `episode_count` | `integer`       | 是   | NULL                | 本次 API 返回的剧集数量, `>= 0`                           |
| `image_url`     | `text`          | 是   | NULL                | `bangumi.imageUrl`                                        |
| `update_at`     | `timestamptz`   | 否   | `CURRENT_TIMESTAMP` | 最后成功同步时间                                          |

`bgm_ani_id` 仅从 `https://bangumi.tv/subject/{非负整数}` URL 提取.
URL 为空时保存 NULL; 非空但格式不符时该动画同步失败.

## public.dandanplay_episode

| 字段         | PostgreSQL 类型               | NULL | 默认值              | API 来源或说明                         |
| ------------ | ----------------------------- | ---- | ------------------- | -------------------------------------- |
| `ddp_epi_id` | `bigint`                      | 否   | 无                  | 主键, `>= 0`; `episode.episodeId`      |
| `ddp_ani_id` | `bigint`                      | 否   | 无                  | 外键引用动画, 删除动画时级联删除剧集   |
| `title`      | `text`                        | 是   | NULL                | `episode.episodeTitle`                 |
| `ep`         | `text`                        | 是   | NULL                | `episode.episodeNumber`, 可能包含字母  |
| `airdate`    | `timestamp without time zone` | 是   | NULL                | `episode.airDate`, 按 API 当地时间保存 |
| `update_at`  | `timestamptz`                 | 否   | `CURRENT_TIMESTAMP` | 最后成功同步时间                       |

普通索引 `dandanplay_episode_lookup_idx (ddp_ani_id, ep)` 用于按动画和集数查询.
不对 `ep` 添加唯一约束, 因为特殊剧集可能有重复或非数字编号.

## 建表 SQL

```sql
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
    ALTER COLUMN ddp_epi_id TYPE bigint USING ddp_epi_id::bigint;
ALTER TABLE public.media_file
    ADD CONSTRAINT dandanplay_media_episode_fk
    FOREIGN KEY (ddp_epi_id)
    REFERENCES public.dandanplay_episode (ddp_epi_id)
    ON UPDATE NO ACTION
    ON DELETE NO ACTION;
```

`ids.ddp_ani_id` 和 `media_file.ddp_epi_id` 均已扩展为 `bigint`.
迁移会重建依赖 `media_file.ddp_epi_id` 的 `view_meida_file`.
实际迁移脚本为 `src/kg/sql/ddp/migrate_schema.sql`.

## 同步行为

- `ddp` 从 `public.ids.ddp_ani_id` 读取非 NULL 且去重排序后的 ID.
- 每个动画独立事务, upsert 动画及其剧集, 刷新 `update_at`.
- API 响应中的动画 ID 必须与请求 ID 相同; ID 和字段类型无效时不写入该动画.
- API 未返回的历史剧集保留.
- 单个动画失败会报告并继续处理后续动画; 最终有失败时返回非零退出码.
- 复用项目全局配置中的 Dandanplay AppID/AppSecret.
- `DANDANPLAY_API_BASE` 默认为 `https://api.dandanplay.net`, 可用于兼容代理或测试服务.
- `REQUEST_TIMEOUT` 默认为 15 秒, `REQUEST_DELAY` 默认为 0.2 秒.
