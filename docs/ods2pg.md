# 从 ODS 文件同步到 PostgreSQL

`ods2pg` 支持命令行显式列映射和 ODS 入口工作表配置块两种模式, 将数据插入或
更新到 PostgreSQL 表. 命令不会创建表, 不会删除未出现在输入文件中的旧记录.

## 用法

```text
cargo run --bin ods2pg -- <ods-file> <sheet-name> <table-name> \
  <pk-column> [x] <pk-name> [<pk-column> [x] <pk-name> ...] \
  - \
  <column> <name> [<column> <name> ...] [--pg-host <host>] \
  [--pg-port <port>] [--pg-user <user>] [--pg-password <password>] \
  [--pg-database <database>]
```

使用一个独立的 `-` 参数分隔主键和普通字段映射. 每侧至少提供一个映射,
主键映射格式为 `ODS 列名 [x] PostgreSQL 列名`, 普通字段映射格式为
`ODS 列名 PostgreSQL 列名`. 支持多个主键字段.
主键的可选标记 `x` 会在非空单元格转换的文本前添加 `\x`,
可用于 PostgreSQL `bytea` 的十六进制输入. 空单元格仍为 SQL NULL.
当 `x` 同时可解释为标记或目标列名时, 优先视为标记.

例如, A 列为十六进制主键:

```bash
cargo run --bin ods2pg -- data.ods Sheet1 public.example \
  A x sha256 - B title
```

例如, A 列作为主键, B 和 C 列作为普通字段:

```bash
cargo run --bin ods2pg -- data.ods Sheet1 public.example \
  A id - B title C score
```

复合主键由 `-` 左侧多个映射构成:

```bash
cargo run --bin ods2pg -- data.ods Sheet1 inventory \
  A store_id B item_id - C description D quantity
```

表名可写作 `table` 或 `schema.table`; 未指定 schema 时使用 `public`.
schema, 表和列标识符按 PostgreSQL 规则引用, 并安全转义双引号.
表名只支持最多一个点号分隔的两部分, 不解析 SQL 双引号语法.

## 配置块模式

配置块模式从 ODS 默认名为 `main` 的入口工作表读取导入定义. 使用 `--main`
可以更改入口工作表. 一次可以按命令行顺序执行多个块:

```text
cargo run --bin ods2pg -- <ods-file> <block-name> [<block-name> ...] \
  [--main <entry-sheet>] [--pg-host <host>] [--pg-port <port>] \
  [--pg-user <user>] [--pg-password <password>] [--pg-database <database>]
```

配置工作表中每条指令占一行, 指令名称位于 A 列, 参数依次放在后续单元格.
以下示例中的 `\t` 表示相邻的 ODS 单元格, 不是单元格里的反斜杠和字母 `t`:

```text
#block       upsert_watch
#table       public.watch
#sheet       episode
#from        5
#to          880
#pk          bgm_epi_id    int           7
#k           watched_at    datetime_tz   27
#k           rating        int           28
#end_block
```

指令说明:

| 指令 | 参数 | 说明 |
| --- | --- | --- |
| `#block` | `<name>` | 开始一个唯一命名的配置块 |
| `#table` | `<table>` 或 `<schema.table>` | 目标普通表或分区表, 省略 schema 时使用 `public` |
| `#sheet` | `<sheet>` | 数据所在工作表 |
| `#from` | `<row>` | 数据起始行, 从 1 开始且包含该行 |
| `#to` | `<row>` | 数据结束边界, 从 1 开始但不包含该行 |
| `#pk` | `<db-column> <type> <source-column>` | 定义冲突主键列, 可重复以声明复合主键 |
| `#k` | `<db-column> <type> <source-column>` | 定义冲突时更新的普通列, 至少一项 |
| `#end_block` | 无 | 结束当前配置块 |

`source-column` 是从 1 开始的十进制列编号, 例如 `1` 是 A 列, `27` 是 AA 列.
`#from 5` 和 `#to 880` 表示处理第 5 至 879 行. 参数必须连续填写,
中间不能留空单元格. 空配置行允许存在.

在同一 ODS 中可以定义多个块:

```bash
cargo run --bin ods2pg -- data.ods upsert_watch upsert_anime --main=entry
```

入口工作表中的所有块都会校验语法和必需字段, 但只有命令行点名的块会读取数据并
连接目标表. 块名称不能重复指定或重复定义. 新旧模式通过是否包含独立的 `-`
分隔符区分, 不应在块名称中使用 `-` 作为独立参数.

### 配置块数据类型

| 类型 | 转换和限制 |
| --- | --- |
| `text` | 字符串原样保留, 其他标量转换为文本 |
| `int` | 必须是 PostgreSQL `bigint` 范围内的整数 |
| `float` | 必须是有限数字 |
| `bool` | 接受布尔单元格以及 `true`, `false`, `1`, `0` |
| `date` | 接受 ODS 日期或 `YYYY-MM-DD`, 写入 ISO 日期 |
| `time` | 接受 ODS 时间或 ISO 时间, 写入 `HH:MM:SS` 格式 |
| `datetime` | 接受 ODS 日期时间或不带时区的 ISO 日期时间 |
| `datetime_tz` | 只接受带 `Z` 或明确 UTC 偏移的 RFC 3339 日期时间 |
| `byte` | 只接受无前缀的偶数位 ASCII 十六进制字符串, 解码后以二进制参数写入 `bytea` |

`byte` 只能映射到 PostgreSQL `bytea` 列. 例如 `00ff10` 会被解码为三个字节,
不是将六个字符 `00ff10` 存成文本. `0x` 和 `\x` 前缀, 空格, 奇数位和非十六进制
字符都会使所在行失败. 空白单元格是 SQL NULL; 空字符串会解码为空的二进制值.

`datetime_tz` 不使用运行机器的本地时区. 没有明确偏移的文本会使所在行失败.
单元格类型转换失败会报告 ODS 工作表, 行列坐标并跳过该数据行, 继续处理其他行.

## 显式映射模式的 ODS 行列规则

- 不输入行范围. 命令从 calamine 返回的工作表实际首行遍历到实际末行.
- 工作表第 1 行没有隐式表头语义, 位于实际范围内时会作为数据行处理.
- 行号用于报错提示时从 1 开始.
- 显式映射模式的列名由一个或多个 ASCII 字母组成, 不区分大小写:
  `A` 是第一列, `Z` 是第二十六列, `AA` 是第二十七列.
- 不根据表头推断映射; 所有 ODS 列和目标数据库列都必须在命令中明确指定.
- 工作表不存在或没有实际单元格范围时, 命令报错且不修改数据库.
- 映射列超出工作表实际列范围时, 命令报错且不修改数据库.
- 一行中所有映射单元格均为空时跳过该行.
- 非全空行中的空单元格写成 SQL NULL. 映射到主键的单元格不得为空.
- 读取单元格或转换为文本失败时, 报告原始行列坐标并跳过所在行.
  calamine 可能将 ODS 公式错误读取为普通文本, 此时由数据库类型或约束校验.
- ODS 文件损坏或工作表本身无法解析时, 无法定位单条记录, 命令会终止.
- 整数, 浮点数, 布尔值, 字符串, 日期/时间和 ISO 日期时间会转换为文本参数,
  再由 PostgreSQL 按目标列类型转换.

配置块模式只处理 `#from` 包含且 `#to` 不包含的行号范围, 并按每个字段声明的
配置类型转换单元格. 范围边界或源列超出工作表实际范围时, 该配置块执行失败.

## 数据库配置

可通过命令选项指定连接参数:

| 选项 | 默认环境变量 | 说明 |
| --- | --- | --- |
| `--pg-host` | `PG_HOST` | PostgreSQL 主机 |
| `--pg-port` | `PG_PORT` | PostgreSQL 端口, 可省略并使用驱动默认端口 |
| `--pg-user` | `PG_USER` | PostgreSQL 用户 |
| `--pg-password` | `PG_PASSWORD` | PostgreSQL 密码 |
| `--pg-database` / `--pg-db` | `PG_DB` | PostgreSQL 数据库名 |

未提供命令行选项时, 从当前目录的 `.env` 和进程环境读取对应变量.
进程中已存在的环境变量优先于 `.env`, 命令行选项优先级最高.
主机, 用户, 密码和数据库名为必填; 端口可选.
命令不会输出密码.

## Upsert 和事务

- 所有 ODS 数据在连接数据库前逐行读出并校验, 全空映射行被过滤.
- 目标必须是指定 schema 下的普通表或分区表, 所有映射列必须存在且可写.
  生成列和 `GENERATED ALWAYS` identity 列不能显式映射.
- 根据 PostgreSQL 系统目录取得目标列真实类型, 再通过参数化 SQL 写入.
  除 `byte` 以二进制参数传递外, 其他值由 PostgreSQL 按目标类型从文本转换.
- `-` 左侧列作为 `ON CONFLICT` 冲突目标. 目标表必须有这些列对应的主键或唯一约束.
- `-` 右侧列仅在冲突时更新; 不会更新主键列.
- 每行先按主键更新已有记录, 只修改映射的普通字段, 保留未映射的字段.
  例如只补充 `mikan_rss.bgm_ani_id` 时, 不需要重新提供已有记录的 `update_datetime`.
- 未找到记录时才插入, 并用 `ON CONFLICT` 处理并发插入冲突.
  新记录仍需满足全部非空约束, 未映射的必填列需要数据库默认值或补充字段映射.
  命令不会自动填充时间, 放宽约束或为缺失主键生成值.
- 每行使用独立事务. 类型转换或约束错误只回滚当前行, 报告原始 ODS 行号和
  数据库错误后继续处理, 包括提交时检查的延迟约束.
- 每行提交时都必须满足约束. 例如外键不能依赖文件中尚未导入的后续行.
- 连接, 目标表或映射等全局错误会终止导入; 已提交的行不会回滚.
- 同一输入中的重复主键按照工作表顺序逐条 upsert, 后一行的普通字段值最终生效.
- 导入结束后报告成功和失败行数. 任何数据行失败时返回非零退出码,
  已成功提交的行仍然保留.
