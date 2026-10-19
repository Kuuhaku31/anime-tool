# Torrent 数据库表结构

## 来源与范围

- 整理日期: 2026-10-19.
- 连接配置来源: 项目根目录的 `.env`.
- 数据来源: PostgreSQL 系统目录和 `psql \d+` 的只读查询结果.
- 范围: `public.torrent` 和 `public.torrent_file` 的字段, 默认值, 约束, 索引和关联关系.
- 查询未读取业务数据, 未修改数据库, 本文不记录连接地址或账号密码.

两张表及其字段均未设置数据库注释. 下文的业务用途是根据命名和项目代码整理的解释,
不代表数据库已强制约束这些语义. 无法确认的字段口径会单独标明.

## 1. public.torrent

保存种子级别的信息, 使用 v1 info hash 唯一标识种子.

### 字段

| 字段              | PostgreSQL 类型         | 允许 NULL | 默认值 | 用途与说明                                                          |
| ----------------- | ----------------------- | --------- | ------ | ------------------------------------------------------------------- |
| `torrent_hash_v1` | `character varying(40)` | 否        | 无     | 种子的 v1 info hash, 主键. 项目使用 20 字节 hash 的小写十六进制文本 |
| `title`           | `text`                  | 是        | 无     | 种子标题                                                            |
| `data`            | `bytea`                 | 是        | 无     | 按项目中的插入示例, 用于保存 `.torrent` 文件原始字节                |
| `size`            | `numeric(100,0)`        | 是        | 无     | 大小相关数值, 具体统计对象和单位未由表结构定义                      |
| `file_size`       | `numeric(100,0)`        | 是        | 无     | 文件大小相关数值, 与 `size` 的区别及单位未由表结构定义              |
| `file_count`      | `numeric(100,0)`        | 是        | 无     | 文件数量, 具体统计口径未由表结构定义                                |
| `folder_count`    | `numeric(100,0)`        | 是        | 无     | 目录数量, 具体统计口径未由表结构定义                                |
| `category`        | `text`                  | 是        | 无     | 种子分类, 项目从 qBittorrent 的 `category` 字段读取                 |

### 约束与索引

| 名称                         | 类别                   | 定义                            |
| ---------------------------- | ---------------------- | ------------------------------- |
| `torrent_pkey`               | 主键及唯一 B-tree 索引 | `PRIMARY KEY (torrent_hash_v1)` |
| `torrent_info_hash_not_null` | 非空约束               | `torrent_hash_v1 NOT NULL`      |

除主键索引外, 当前没有其他索引.

## 2. public.torrent_file

保存种子中的文件路径和可选的 SHA-256 值.
使用种子 hash 与相对路径的组合唯一标识一条文件记录.

### 字段

| 字段              | PostgreSQL 类型         | 允许 NULL | 默认值 | 用途与说明                                      |
| ----------------- | ----------------------- | --------- | ------ | ----------------------------------------------- |
| `torrent_hash_v1` | `character varying(40)` | 否        | 无     | 所属种子的 v1 info hash, 同时属于联合主键和外键 |
| `relative_path`   | `text`                  | 否        | 无     | 文件在种子内的相对路径, 属于联合主键            |
| `sha256`          | `character varying(64)` | 是        | 无     | 文件 SHA-256 值的文本表示, 允许尚未填写         |

### 约束与索引

| 名称                                    | 类别                       | 定义                                                                       |
| --------------------------------------- | -------------------------- | -------------------------------------------------------------------------- |
| `torrent_file_pkey`                     | 联合主键及唯一 B-tree 索引 | `PRIMARY KEY (torrent_hash_v1, relative_path)`                             |
| `fk_torrent_file_torrent`               | 外键                       | `FOREIGN KEY (torrent_hash_v1) REFERENCES public.torrent(torrent_hash_v1)` |
| `torrent_file_torrent_hash_v1_not_null` | 非空约束                   | `torrent_hash_v1 NOT NULL`                                                 |
| `torrent_file_relative_path_not_null`   | 非空约束                   | `relative_path NOT NULL`                                                   |

除联合主键索引外, 当前没有其他索引, `sha256` 没有索引或唯一约束.
联合主键以 `torrent_hash_v1` 为首列, 可以支持按种子 hash 查找文件.

## 3. 表间关系

```text
public.torrent
  PRIMARY KEY (torrent_hash_v1)
       |
       | 1 : 0..N
       v
public.torrent_file
  FOREIGN KEY (torrent_hash_v1)
  PRIMARY KEY (torrent_hash_v1, relative_path)
```

- 一条种子记录可以没有文件记录, 也可以关联多条文件记录.
- 每条文件记录必须引用一条已存在的种子记录.
- 同一种子内不能重复保存相同的 `relative_path`, 不同种子可以使用相同路径.
- 外键采用默认的 `ON UPDATE NO ACTION` 和 `ON DELETE NO ACTION`, 没有级联更新或删除.
- 存在关联文件时, 直接删除种子或修改其主键会受到外键约束限制.
- 当前系统目录中, 涉及这两张表的外键仅有 `fk_torrent_file_torrent`.

## 4. 当前结构对应的建表 SQL

以下 SQL 用于说明当前基础表结构, 保留查询到的约束名称.
它不是迁移脚本, 不包含所有者, 权限和部署环境配置, 不应直接在已有表上执行.

```sql
-- 保存种子级别的信息.
CREATE TABLE public.torrent (
    -- v1 info hash 文本, 唯一标识种子.
    torrent_hash_v1 character varying(40)
        CONSTRAINT torrent_info_hash_not_null NOT NULL,
    -- 种子标题.
    title text,
    -- 种子文件的原始二进制数据.
    data bytea,
    -- 大小相关数值, 统计对象和单位待确认.
    size numeric(100,0),
    -- 文件大小相关数值, 统计对象和单位待确认.
    file_size numeric(100,0),
    -- 文件数量, 统计口径待确认.
    file_count numeric(100,0),
    -- 目录数量, 统计口径待确认.
    folder_count numeric(100,0),
    -- 种子分类.
    category text,
    CONSTRAINT torrent_pkey PRIMARY KEY (torrent_hash_v1)
);

-- 保存种子中的文件信息.
CREATE TABLE public.torrent_file (
    -- 所属种子的 v1 info hash 文本.
    torrent_hash_v1 character varying(40)
        CONSTRAINT torrent_file_torrent_hash_v1_not_null NOT NULL,
    -- 文件在种子内的相对路径.
    relative_path text
        CONSTRAINT torrent_file_relative_path_not_null NOT NULL,
    -- 文件的 SHA-256 文本, 可暂不填写.
    sha256 character varying(64),
    CONSTRAINT torrent_file_pkey PRIMARY KEY (torrent_hash_v1, relative_path),
    CONSTRAINT fk_torrent_file_torrent
        FOREIGN KEY (torrent_hash_v1)
        REFERENCES public.torrent (torrent_hash_v1)
);
```

主键会自动创建上文列出的同名唯一 B-tree 索引, 不需要重复创建.

## 5. 使用注意事项

- `qb-info` 使用单个事务先写入种子再写入文件. 种子冲突时仅更新 `title` 和 `category`,
  文件冲突时不做修改. 不更新 `data`, 大小, 计数或 `sha256`, 也不删除已有记录.
- `title` 和 `category` 为缺失值时会写入 NULL, 包括覆盖数据库中的已有值.
- `torrent_hash_v1` 存储的是文本, 不是 `bytea`. 二进制 hash 需要先编码成十六进制文本再比较.
- `VARCHAR(40)` 和 `VARCHAR(64)` 只限制最大长度, 不保证恰好 40 或 64 个字符,
  也不校验十六进制字符或统一大小写. 当前没有相应的 `CHECK` 约束.
- `relative_path` 只受非空约束限制, 没有空字符串, 路径规范或目录穿越相关的 `CHECK` 约束.
- `numeric(100,0)` 是最多 100 位整数精度的数值类型, 并非 `bigint`.
  当前没有非负值约束, 也没有大小与计数字段之间的一致性约束.
- `sha256` 可以为 NULL, 也没有唯一约束, 不能仅凭该字段假设文件记录唯一.
- 任何字段都没有显式默认值. 插入时省略可空字段会得到 NULL,
  省略必填字段则会违反非空约束.
