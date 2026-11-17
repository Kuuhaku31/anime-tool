
# Bangumi 动画和剧集表结构

`public.bangumi_anime` 保存 Bangumi 动画字段, `public.bangumi_episode`
保存动画对应的剧集. 字段取自旧项目 `cache_bgm_pkg_to_db.py` 和
`a.sql`, 仅保留当前业务使用的数据.

## public.bangumi_anime

| 字段 | PostgreSQL 类型 | NULL | 默认值 | 约束或来源 |
| --- | --- | --- | --- | --- |
| `bgm_ani_id` | `integer` | 否 | 无 | 主键, `>= 0`; `subject.id` |
| `title` | `text` | 是 | NULL | `subject.name` |
| `title_cn` | `text` | 是 | NULL | `subject.name_cn` |
| `airdate` | `date` | 是 | NULL | `subject.date`, 空字符串写为 NULL |
| `summary` | `text` | 是 | NULL | `subject.summary` |
| `ep_count` | `integer` | 是 | NULL | `>= 0`; `subject.eps` |
| `object_count` | `integer` | 是 | NULL | `>= 0`; `subject.total_episodes` |
| `cover_url` | `text` | 是 | NULL | `subject.images.large` |
| `official_site` | `text` | 是 | NULL | `infobox` 中 `key = "官方网站"` 的字符串值 |
| `update_at` | `timestamptz` | 否 | `CURRENT_TIMESTAMP` | 每次同步 upsert 时刷新 |

## public.bangumi_episode

| 字段 | PostgreSQL 类型 | NULL | 默认值 | 约束或来源 |
| --- | --- | --- | --- | --- |
| `bgm_epi_id` | `integer` | 否 | 无 | 主键, `>= 0`; `episode.id` |
| `bgm_ani_id` | `integer` | 否 | 无 | 外键引用动画, 删除动画时级联删除剧集 |
| `title` | `text` | 是 | NULL | `episode.name` |
| `title_cn` | `text` | 是 | NULL | `episode.name_cn` |
| `airdate` | `date` | 是 | NULL | `episode.airdate`, 空字符串写为 NULL |
| `summary` | `text` | 是 | NULL | `episode.desc` |
| `ep` | `text` | 是 | NULL | API `episode.sort` 的文本表示, 保留小数或文本 |
| `sort` | `smallint` | 是 | NULL | `>= 0`; API `episode.ep` 的整数值 |
| `duration` | `integer` | 是 | NULL | `>= 0`; `episode.duration_seconds` |
| `update_at` | `timestamptz` | 否 | `CURRENT_TIMESTAMP` | 每次同步 upsert 时刷新 |

剧集表额外有索引 `bangumi_episode_bgm_ani_id_idx (bgm_ani_id)`,
用于按动画查询剧集. 不保存 API 原始 JSON, 评分, 收藏统计, 标签或其他尺寸封面.

**特别说明: 数据库与 Bangumi API 的 ep 和 sort 不是同名映射.**
数据库 `ep` 取自 API `episode.sort`, 转为文本以保留如 `12.5` 的集数.
数据库 `sort` 取自 API `episode.ep`, 使用 `smallint`, 不做截断或四舍五入.
API `episode.ep` 为小数或超出 smallint 范围时, 当前动画报错且不写入.

## 建表 SQL

```sql
CREATE TABLE public.bangumi_anime (
    bgm_ani_id integer PRIMARY KEY,
    title text,
    title_cn text,
    airdate date,
    summary text,
    ep_count integer,
    object_count integer,
    cover_url text,
    official_site text,
    update_at timestamp with time zone NOT NULL DEFAULT CURRENT_TIMESTAMP,
    CONSTRAINT bangumi_anime_id_positive CHECK (bgm_ani_id >= 0),
    CONSTRAINT bangumi_anime_ep_count_nonnegative
        CHECK (ep_count IS NULL OR ep_count >= 0),
    CONSTRAINT bangumi_anime_object_count_nonnegative
        CHECK (object_count IS NULL OR object_count >= 0)
);

CREATE TABLE public.bangumi_episode (
    bgm_epi_id integer PRIMARY KEY,
    bgm_ani_id integer NOT NULL,
    title text,
    title_cn text,
    airdate date,
    summary text,
    ep text,
    sort smallint,
    duration integer,
    update_at timestamp with time zone NOT NULL DEFAULT CURRENT_TIMESTAMP,
    CONSTRAINT bangumi_episode_id_positive CHECK (bgm_epi_id >= 0),
    CONSTRAINT bangumi_episode_anime_fk
        FOREIGN KEY (bgm_ani_id)
        REFERENCES public.bangumi_anime (bgm_ani_id)
        ON DELETE CASCADE,
    CONSTRAINT bangumi_episode_sort_nonnegative
        CHECK (sort IS NULL OR sort >= 0),
    CONSTRAINT bangumi_episode_duration_nonnegative
        CHECK (duration IS NULL OR duration >= 0)
);

CREATE INDEX bangumi_episode_bgm_ani_id_idx
    ON public.bangumi_episode (bgm_ani_id);
```

## 同步行为

- `bgm` 从 `public.ids.bgm_ani_id` 读取非 NULL 且去重的动画 ID.
- 每个动画独立事务, 先写动画再写剧集. 剧集 ID 缺失或无效时跳过该条目.
- 重复 ID 使用 upsert 更新数据和 `update_at`; 不删除 API 本次未返回的旧剧集.
- 一个动画的 API 分页完整获取后才开始写入. 该动画写入失败时整笔事务回滚,
  程序继续处理后续动画.
- Bangumi 日期按 `date` 写入. 支持标准 `YYYY-MM-DD` 和旧缓存中的
  `YYYY-M-D` 日期文本.
- 数据库 `main` 中的表由部署或数据库管理步骤创建, `bgm` 不自动修改表结构.
