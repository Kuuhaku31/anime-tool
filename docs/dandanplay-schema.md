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
上述 SQL 记录了原迁移. 当前数据库的 `media_file.ddp_epi_id` 已无剧集外键,
检查约束为 `ddp_epi_id >= -1`, 其中 `-1` 表示无法精确关联.

## public.dandanplay_danmaku

`public.dandanplay_danmaku` 每行保存一个剧集弹幕, 来自
`GET /api/v2/comment/{episodeId}`. 官方 `cid` 是 int64,
`p` 格式为 `出现秒数,模式,颜色,用户ID`, `m` 是弹幕正文.

| 字段 | PostgreSQL 类型 | NULL | 默认值 | 说明 |
| --- | --- | --- | --- | --- |
| `ddp_epi_id` | `bigint` | 否 | 无 | 所属剧集, 复合主键首列 |
| `cid` | `bigint` | 否 | 无 | API 弹幕 ID, 复合主键次列 |
| `appear_time` | `double precision` | 是 | NULL | 从 `p` 拆出的出现秒数 |
| `mode` | `smallint` | 否 | `1` | `1` 滚动, `4` 底部, `5` 顶部 |
| `color` | `integer` | 否 | `16777215` | RGB 整数, 默认白色 |
| `user_id` | `text` | 是 | NULL | 从 `p` 拆出, 按文本保存 |
| `content` | `text` | 是 | NULL | API 字段 `m` |
| `raw_p` | `text` | 是 | NULL | 保留 API 原始 `p` |
| `update_at` | `timestamptz` | 否 | `CURRENT_TIMESTAMP` | 最近一次同步时间 |

主键为 `(ddp_epi_id, cid)`, 避免假设关联弹幕在不同剧集间 ID 全局唯一.
剧集外键使用 `ON DELETE CASCADE`. 时间线索引为
`(ddp_epi_id, appear_time, cid)`. `p` 无效时保留 `raw_p`,
`appear_time` 和 `user_id` 写 NULL, `mode` 和 `color` 使用默认值.

```sql
CREATE TABLE public.dandanplay_danmaku (
    ddp_epi_id bigint NOT NULL,
    cid bigint NOT NULL,
    appear_time double precision,
    mode smallint NOT NULL DEFAULT 1,
    color integer NOT NULL DEFAULT 16777215,
    user_id text,
    content text,
    raw_p text,
    update_at timestamp with time zone NOT NULL DEFAULT CURRENT_TIMESTAMP,
    CONSTRAINT dandanplay_danmaku_pkey PRIMARY KEY (ddp_epi_id, cid),
    CONSTRAINT dandanplay_danmaku_episode_fk
        FOREIGN KEY (ddp_epi_id)
        REFERENCES public.dandanplay_episode (ddp_epi_id)
        ON UPDATE NO ACTION
        ON DELETE CASCADE,
    CONSTRAINT dandanplay_danmaku_cid_nonnegative CHECK (cid >= 0),
    CONSTRAINT dandanplay_danmaku_time_range
        CHECK (appear_time IS NULL OR appear_time >= 0),
    CONSTRAINT dandanplay_danmaku_mode_range CHECK (mode IN (1, 4, 5)),
    CONSTRAINT dandanplay_danmaku_color_range
        CHECK (color BETWEEN 0 AND 16777215)
);

CREATE INDEX dandanplay_danmaku_timeline_idx
    ON public.dandanplay_danmaku (ddp_epi_id, appear_time, cid);
```

实际建表 SQL 为 `src/kg/sql/ddp/create_danmaku.sql`.
命令 `cargo run --bin ddp-danmaku-db -- <ddp_epi_id>` 同步单集弹幕.
它按 `(ddp_epi_id, cid)` upsert, 更新已有弹幕但不删除 API 本次未返回的记录.

## 同步行为

- `ddp` 从 `public.ids.ddp_ani_id` 读取非 NULL 且去重排序后的 ID.
- 每个动画独立事务, upsert 动画及其剧集, 刷新 `update_at`.
- API 响应中的动画 ID 必须与请求 ID 相同; ID 和字段类型无效时不写入该动画.
- API 未返回的历史剧集保留.
- 单个动画失败会报告并继续处理后续动画; 最终有失败时返回非零退出码.
- 复用项目全局配置中的 Dandanplay AppID/AppSecret.
- `DANDANPLAY_API_BASE` 默认为 `https://api.dandanplay.net`, 可用于兼容代理或测试服务.
- `REQUEST_TIMEOUT` 默认为 15 秒, `REQUEST_DELAY` 默认为 0.2 秒.
