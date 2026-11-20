# 从 ODS 文件同步到 PostgreSQL

`ods2pg` 从 ODS 工作表读取实际使用范围, 按显式列映射将数据插入或更新到
PostgreSQL 表. 命令不会创建表, 不会删除未出现在输入文件中的旧记录.

## 用法

```text
cargo run --bin ods2pg -- <ods-file> <sheet-name> <table-name> \
  <pk-column> <pk-name> [<pk-column> <pk-name> ...] \
  - \
  <column> <name> [<column> <name> ...] [--pg-host <host>] \
  [--pg-port <port>] [--pg-user <user>] [--pg-password <password>] \
  [--pg-database <database>]
```

使用一个独立的 `-` 参数分隔主键和普通字段映射. 两侧映射均由
`ODS 列名 PostgreSQL 列名` 参数对组成, 且都必须至少提供一对.
支持多个主键字段.

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

## ODS 行列规则

- 不输入行范围. 命令从 calamine 返回的工作表实际首行遍历到实际末行.
- 工作表第 1 行没有隐式表头语义, 位于实际范围内时会作为数据行处理.
- 行号用于报错提示时从 1 开始.
- 列名由一个或多个 ASCII 字母组成, 不区分大小写:
  `A` 是第一列, `Z` 是第二十六列, `AA` 是第二十七列.
- 不根据表头推断映射; 所有 ODS 列和目标数据库列都必须在命令中明确指定.
- 工作表不存在或没有实际单元格范围时, 命令报错且不修改数据库.
- 映射列超出工作表实际列范围时, 命令报错且不修改数据库.
- 一行中所有映射单元格均为空时跳过该行.
- 非全空行中的空单元格写成 SQL NULL. 映射到主键的单元格不得为空.
- ODS 错误单元格会报告原始行列坐标并终止整个导入.
- 整数, 浮点数, 布尔值, 字符串, 日期/时间和 ISO 日期时间会转换为文本参数,
  再由 PostgreSQL 按目标列类型转换.

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

- 所有 ODS 数据在连接数据库前读出并校验, 全空映射行被过滤.
- 目标必须是指定 schema 下的普通表或分区表, 所有映射列必须存在且可写.
  生成列和 `GENERATED ALWAYS` identity 列不能显式映射.
- 根据 PostgreSQL 系统目录取得目标列真实类型, 再通过参数化 SQL 转换单元格文本.
  类型转换由数据库执行, 支持 PostgreSQL 类型系统能够从文本输入的类型.
- `-` 左侧列作为 `ON CONFLICT` 冲突目标. 目标表必须有这些列对应的主键或唯一约束.
- `-` 右侧列仅在冲突时更新; 不会更新主键列.
- 全部行在同一个事务内写入. 任何数据类型, 非空约束, 检查约束, 外键或
  唯一约束错误都会回滚本次导入, 并报告对应的原始 ODS 行号和数据库错误.
- 同一输入中的重复主键按照工作表顺序逐条 upsert, 后一行的普通字段值最终生效.
- 成功后报告 ODS 文件, 工作表, 目标表和实际写入行数.
