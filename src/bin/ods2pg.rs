
//! src/bin/ods2pg.rs
//! 按 ODS 工作表列映射, 逐行解析并独立 upsert 到 PostgreSQL.

use std::{
    collections::HashSet,
    env,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, bail};
use calamine::{Data, Ods, Reader, open_workbook};
use clap::Parser;
use postgres::{Client, NoTls, Transaction, types::ToSql};

/// 命令行参数和分隔符之后的映射及连接选项.
#[derive(Parser)]
#[command(
    name = "ods2pg",
    version,
    about = "从 ODS 文件按列映射 upsert PostgreSQL 表",
    after_help = "数据库选项可放在映射参数任意位置: --pg-host, --pg-port, --pg-user, --pg-password, --pg-database"
)]
struct Cli {
    /// ODS 文件路径.
    ods_file: PathBuf,

    /// ODS 工作表名称.
    sheet_name: String,

    /// 目标表名称, 支持 table 或 schema.table.
    table_name: String,

    /// 主键和普通字段映射, 由单独的 "-" 分隔符分开.
    #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
    tail: Vec<String>,
}

/// 数据库连接配置, 显式命令行选项优先.
#[derive(Default)]
struct PgOptions {
    host: Option<String>,      // PostgreSQL 主机.
    port: Option<String>,      // PostgreSQL 端口.
    user: Option<String>,      // PostgreSQL 用户.
    password: Option<String>,  // PostgreSQL 密码.
    database: Option<String>,  // PostgreSQL 数据库.
}

/// 一列 ODS 数据到目标数据库列的映射.
struct ColumnMapping {
    ods_column: usize,    // 从 0 开始的 ODS 列索引.
    db_column: String,    // PostgreSQL 目标列名.
    prefix_hex: bool,     // 非空值是否添加 PostgreSQL 十六进制前缀.
}

/// 主键映射与普通字段映射.
struct Mappings {
    primary_keys: Vec<ColumnMapping>,  // 冲突目标列.
    update_fields: Vec<ColumnMapping>, // 冲突时更新的非主键列.
}

/// 已与 PostgreSQL 目录类型关联的单列映射.
struct ResolvedColumnMapping {
    db_column: String,    // PostgreSQL 目标列名.
    pg_type: String,      // PostgreSQL 目录返回的类型表达式.
}

/// 经过目标列检查的完整写入映射.
struct ResolvedMappings {
    primary_keys: Vec<ResolvedColumnMapping>,  // 冲突目标列.
    update_fields: Vec<ResolvedColumnMapping>, // 冲突时更新的非主键列.
}

/// 一个已验证 ODS 非空映射行及其原始工作表行号.
struct OdsRow {
    row_number: usize,            // 从 1 开始的工作表行号.
    values: Vec<Option<String>>,  // 主键字段后接普通字段的文本值.
}


/// 执行命令并在失败时输出完整错误链.
fn main() {
    if let Err(error) = run() {
        eprintln!("ODS 同步失败: {error:#}");
        std::process::exit(1);
    }
}

/// 读取 ODS 并逐行导入, 最后报告成功和失败数量.
fn run() -> Result<()> {
    let cli = Cli::parse();
    load_dotenv()?;
    let table = parse_table_name(&cli.table_name)?;
    let (positional, pg_options) = split_pg_options(cli.tail)?;
    let mappings = parse_mappings(&positional)?;
    let (rows, parse_failures) = read_ods_rows(&cli.ods_file, &cli.sheet_name, &mappings)?;
    let mut db = connect_postgres(pg_options)?;
    let (count, write_failures) = upsert_rows(&mut db, &table.0, &table.1, &mappings, &rows)?;
    let failures = parse_failures + write_failures;

    println!(
        "从 {} 工作表 {:?} 同步到 {}.{}: 成功 {} 行, 失败 {} 行",
        cli.ods_file.display(), cli.sheet_name, table.0, table.1, count, failures
    );
    if failures > 0 {
        bail!("部分 ODS 数据行导入失败");
    }
    Ok(())
}

/// 分离 "--pg-*" 选项, 保留其余映射参数顺序.
fn split_pg_options(args: Vec<String>) -> Result<(Vec<String>, PgOptions)> {
    let mut positional = Vec::new();
    let mut options = PgOptions::default();
    let mut index = 0;
    while index < args.len() {
        let arg = &args[index];
        let (name, inline_value) = arg
            .split_once('=')
            .map_or((arg.as_str(), None), |(name, value)| (name, Some(value)));
        let slot = match name {
            "--pg-host" => Some(&mut options.host),
            "--pg-port" => Some(&mut options.port),
            "--pg-user" => Some(&mut options.user),
            "--pg-password" => Some(&mut options.password),
            "--pg-database" | "--pg-db" => Some(&mut options.database),
            _ => None,
        };
        if let Some(slot) = slot {
            if slot.is_some() {
                bail!("命令行选项 {name} 不能重复指定");
            }
            let value = if let Some(value) = inline_value {
                if value.is_empty() {
                    bail!("命令行选项 {name} 的值不能为空");
                }
                value.to_owned()
            } else {
                index += 1;
                let value = args.get(index).with_context(|| format!("命令行选项 {name} 缺少值"))?;
                if value.starts_with("--") {
                    bail!("命令行选项 {name} 缺少值");
                }
                value.clone()
            };
            *slot = Some(value);
        } else if arg.starts_with("--pg-") {
            bail!("未知数据库选项: {name}");
        } else {
            positional.push(arg.clone());
        }
        index += 1;
    }
    Ok((positional, options))
}

/// 解析连字符两侧的 ODS 列和数据库字段映射.
fn parse_mappings(args: &[String]) -> Result<Mappings> {
    let separators = args.iter().enumerate().filter(|(_, arg)| *arg == "-").map(|(i, _)| i).collect::<Vec<_>>();
    if separators.len() != 1 {
        bail!("必须且只能提供一个独立的 \"-\" 分隔符");
    }
    let separator = separators[0];
    if separator == 0 || separator + 1 == args.len() {
        bail!("\"-\" 两侧都必须提供字段映射");
    }
    let primary_args = &args[..separator];
    let update_args = &args[separator + 1..];
    if primary_args.is_empty() || update_args.is_empty() {
        bail!("至少提供一个主键映射和一个普通字段映射");
    }

    let mut source_columns = HashSet::new();
    let mut target_columns = HashSet::new();
    let primary_keys = parse_mapping_pairs(primary_args, true, &mut source_columns, &mut target_columns)?;
    let update_fields = parse_mapping_pairs(update_args, false, &mut source_columns, &mut target_columns)?;
    Ok(Mappings { primary_keys, update_fields })
}

/// 解析映射及主键可选的 x 标记, 并拒绝重复 ODS 或数据库列.
fn parse_mapping_pairs(
    args: &[String],
    primary: bool,
    source_columns: &mut HashSet<usize>,
    target_columns: &mut HashSet<String>,
) -> Result<Vec<ColumnMapping>> {
    // 从后向前确认剩余参数能否构成完整映射, 允许末尾的目标列名为 x.
    let mut valid = vec![false; args.len() + 1];
    valid[args.len()] = true;
    for index in (0..args.len()).rev() {
        if parse_column_name(&args[index]).is_err() {
            continue;
        }
        let name_valid = |name: &str| !name.is_empty() && !name.contains('\0');
        valid[index] = (index + 2 <= args.len()
            && name_valid(&args[index + 1]) && valid[index + 2])
            || (primary && index + 3 <= args.len() && args[index + 1] == "x"
                && name_valid(&args[index + 2]) && valid[index + 3]);
    }
    if !valid[0] {
        bail!("映射格式必须为 ODS 列名 [x] 数据库列名, 普通字段不支持 x 标记");
    }
    let mut mappings = Vec::new();
    let mut index = 0;
    while index < args.len() {
        let prefix_hex = primary && index + 3 <= args.len() && args[index + 1] == "x"
            && !args[index + 2].is_empty() && !args[index + 2].contains('\0')
            && valid[index + 3];
        let name_index = index + if prefix_hex { 2 } else { 1 };
        let ods_column = parse_column_name(&args[index])
            .with_context(|| format!("无效的 ODS 列名: {:?}", args[index]))?;
        if !source_columns.insert(ods_column) {
            bail!("ODS 列 {} 被重复映射", args[index]);
        }
        if !target_columns.insert(args[name_index].clone()) {
            bail!("PostgreSQL 列 {} 被重复映射", args[name_index]);
        }
        mappings.push(ColumnMapping {
            ods_column,
            db_column: args[name_index].clone(),
            prefix_hex,
        });
        index = name_index + 1;
    }
    Ok(mappings)
}

/// 将 ASCII 字母列名转换为从 0 开始的列索引.
fn parse_column_name(name: &str) -> Result<usize> {
    if name.is_empty() || !name.bytes().all(|byte| byte.is_ascii_alphabetic()) {
        bail!("列名只能包含 ASCII 字母");
    }
    let mut value = 0usize;
    for byte in name.bytes() {
        let digit = usize::from(byte.to_ascii_uppercase() - b'A') + 1;
        value = value.checked_mul(26).and_then(|n| n.checked_add(digit))
            .context("ODS 列索引超出平台范围")?;
    }
    value.checked_sub(1).context("ODS 列索引无效")
}

/// 校验并拆分 table 或 schema.table 格式的目标表名.
fn parse_table_name(name: &str) -> Result<(String, String)> {
    if name.contains('\0') {
        bail!("表名不能包含 NUL 字符");
    }
    let mut parts = name.split('.');
    let first = parts.next().unwrap_or_default();
    let second = parts.next();
    if first.is_empty() || second.is_some_and(str::is_empty) || parts.next().is_some() {
        bail!("表名必须是 table 或 schema.table, 且各部分不能为空");
    }
    match second {
        Some(table) => Ok((first.to_owned(), table.to_owned())),
        None => Ok(("public".to_owned(), first.to_owned())),
    }
}

/// 打开指定 ODS 工作表并自动遍历 calamine 返回的实际使用范围.
fn read_ods_rows(
    path: &Path,
    sheet_name: &str,
    mappings: &Mappings,
) -> Result<(Vec<OdsRow>, usize)> {
    if !path.is_file() {
        bail!("ODS 路径不是普通文件: {}", path.display());
    }
    let mut workbook: Ods<_> = open_workbook(path)
        .with_context(|| format!("打开 ODS 文件失败: {}", path.display()))?;
    let range = workbook.worksheet_range(sheet_name)
        .with_context(|| format!("读取 ODS 工作表失败: {sheet_name:?}"))?;
    let (first_row, first_column) = range.start()
        .with_context(|| format!("工作表 {sheet_name:?} 没有实际单元格范围"))?;
    let (last_row, last_column) = range.end()
        .with_context(|| format!("工作表 {sheet_name:?} 没有实际单元格范围"))?;
    let max_column = mappings.primary_keys.iter().chain(&mappings.update_fields)
        .map(|mapping| mapping.ods_column)
        .max()
        .context("没有可读取的 ODS 列映射")?;
    let max_column = u32::try_from(max_column).context("ODS 列索引超出坐标范围")?;
    if max_column > last_column {
        bail!(
            "映射的 ODS 列索引 {} 超出工作表实际列范围 {}..={}",
            max_column + 1,
            first_column + 1,
            last_column + 1
        );
    }
    for mapping in mappings.primary_keys.iter().chain(&mappings.update_fields) {
        let column = u32::try_from(mapping.ods_column).context("ODS 列索引超出坐标范围")?;
        if column < first_column {
            bail!(
                "映射的 ODS 列 {} 超出工作表实际列范围 {}..={}",
                column + 1,
                first_column + 1,
                last_column + 1
            );
        }
    }

    let row_count = usize::try_from(last_row - first_row)
        .context("工作表行范围超出平台容量")?
        .checked_add(1)
        .context("工作表行数超出平台容量")?;
    let mut rows = Vec::with_capacity(row_count);
    let mut failures = 0;
    for row_index in first_row..=last_row {
        let values = mappings.primary_keys.iter().chain(&mappings.update_fields)
            .map(|mapping| {
                let column_index = u32::try_from(mapping.ods_column).context("ODS 列索引超出坐标范围")?;
                let cell = range.get_value((row_index, column_index))
                    .with_context(|| format!("ODS 第 {} 行第 {} 列超出工作表范围", row_index + 1, column_index + 1))?;
                let value = cell_to_text(cell)
                    .with_context(|| format!("读取 ODS 第 {} 行第 {} 列失败", row_index + 1, column_index + 1))?;
                Ok(value.map(|value| {
                    if mapping.prefix_hex { format!("\\x{value}") } else { value }
                }))
            })
            .collect::<Result<Vec<_>>>();
        let values = match values {
            Ok(values) => values,
            Err(error) => {
                eprintln!("{error:#}");
                failures += 1;
                continue;
            }
        };
        if values.iter().all(Option::is_none) {
            continue;
        }
        rows.push(OdsRow {
            row_number: row_index as usize + 1,
            values,
        });
    }
    Ok((rows, failures))
}

/// 将 ODS 单元格转换为数据库文本参数, 空值保留为 SQL NULL.
fn cell_to_text(cell: &Data) -> Result<Option<String>> {
    match cell {
        Data::Empty => Ok(None),
        Data::Int(value) => Ok(Some(value.to_string())),
        Data::Float(value) if value.is_finite() && value.fract() == 0.0 => {
            Ok(Some(format!("{value:.0}")))
        }
        Data::Float(value) => Ok(Some(value.to_string())),
        Data::String(value) => Ok(Some(value.clone())),
        Data::Bool(value) => Ok(Some(value.to_string())),
        Data::DateTime(value) => value.as_datetime()
            .map(|datetime| Some(datetime.to_string()))
            .with_context(|| format!("无法转换 ODS 日期时间单元格 {value:?}")),
        Data::DateTimeIso(value) | Data::DurationIso(value) => Ok(Some(value.clone())),
        Data::Error(error) => bail!("ODS 单元格包含错误值: {error:?}"),
    }
}

/// 读取当前目录 .env, 保留进程中已设置的环境变量.
fn load_dotenv() -> Result<()> {
    match dotenvy::from_path(".env") {
        Ok(_) => Ok(()),
        Err(dotenvy::Error::Io(error)) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error).context("加载当前目录 .env 失败"),
    }
}

/// 使用 CLI 覆盖项和 PG_* 环境变量连接目标数据库.
fn connect_postgres(options: PgOptions) -> Result<Client> {
    fn required(name: &str, cli_value: Option<String>) -> Result<String> {
        let value = match cli_value {
            Some(value) => value,
            None => env::var(name).with_context(|| format!("请通过命令行或环境变量设置 {name}"))?,
        };
        if value.is_empty() {
            bail!("{name} 不能为空");
        }
        Ok(value)
    }

    let mut config = postgres::Config::new();
    config.host(&required("PG_HOST", options.host)?)
        .user(&required("PG_USER", options.user)?)
        .password(required("PG_PASSWORD", options.password)?)
        .dbname(&required("PG_DB", options.database)?);
    let port = match options.port {
        Some(value) => Some(value),
        None => match env::var("PG_PORT") {
            Ok(value) => Some(value),
            Err(env::VarError::NotPresent) => None,
            Err(error) => return Err(error).context("读取 PG_PORT 失败"),
        },
    };
    if let Some(value) = port {
        let port = value.parse::<u16>().context("PG_PORT 必须是 1 到 65535 的整数")?;
        if port == 0 {
            bail!("PG_PORT 不能为 0");
        }
        config.port(port);
    }
    config.connect(NoTls).context("连接 PostgreSQL 失败")
}

/// 引用 PostgreSQL 标识符并转义内部双引号.
fn quote_identifier(identifier: &str) -> String {
    format!("\"{}\"", identifier.replace('"', "\"\""))
}

/// 以目录中的目标列类型生成 upsert SQL, 每行单独提交并记录失败.
fn upsert_rows(
    db: &mut Client,
    schema: &str,
    table: &str,
    mappings: &Mappings,
    rows: &[OdsRow],
) -> Result<(u64, usize)> {
    let qualified_table = format!("{}.{}", quote_identifier(schema), quote_identifier(table));
    let mut transaction = db.transaction().context("开始 ODS 导入事务失败")?;
    transaction.batch_execute(&format!("LOCK TABLE {qualified_table} IN ROW EXCLUSIVE MODE"))
        .with_context(|| format!("锁定目标表 {schema}.{table} 失败"))?;
    let columns = load_column_types(&mut transaction, schema, table)?;
    let resolved = mappings.clone_with_types(&columns)?;
    let sql = build_upsert_sql(schema, table, &resolved);
    transaction.commit().context("提交目标表检查事务失败")?;
    let statement = db.prepare(&sql).context("准备 upsert SQL 失败")?;
    let mut total = 0u64;
    let mut failures = 0;
    for row in rows {
        if mappings.primary_keys.iter().enumerate().any(|(index, _)| row.values[index].is_none()) {
            eprintln!("ODS 第 {} 行的主键映射不能为空", row.row_number);
            failures += 1;
            continue;
        }
        let params = row.values.iter().map(|value| value as &(dyn ToSql + Sync)).collect::<Vec<_>>();
        let mut row_transaction = db.transaction().context("开始 ODS 数据行事务失败")?;
        let result = match row_transaction.execute(&statement, &params) {
            Ok(count) => row_transaction.commit().map(|()| count),
            Err(error) => {
                row_transaction.rollback().context("回滚 ODS 数据行事务失败")?;
                Err(error)
            }
        };
        match result {
            Ok(count) => total += count,
            Err(error) => {
                if let Some(database_error) = error.as_db_error() {
                    eprintln!("写入 ODS 第 {} 行失败: {}", row.row_number, database_error.message());
                    if let Some(detail) = database_error.detail() {
                        eprintln!("数据库详情: {detail}");
                    }
                    failures += 1;
                } else {
                    return Err(error).with_context(|| format!("写入 ODS 第 {} 行时数据库连接或协议失败", row.row_number));
                }
            }
        }
    }
    Ok((total, failures))
}

/// 获取目标表中所有映射列的完整类型信息.
fn load_column_types(
    transaction: &mut Transaction<'_>,
    schema: &str,
    table: &str,
) -> Result<std::collections::HashMap<String, (String, bool)>> {
    let rows = transaction.query(
        "SELECT a.attname, pg_catalog.format_type(a.atttypid, a.atttypmod), \
                (a.attgenerated <> '') AS generated, a.attidentity = 'a' AS identity_always \
         FROM pg_catalog.pg_class c \
         JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace \
         JOIN pg_catalog.pg_attribute a ON a.attrelid = c.oid \
         WHERE n.nspname = $1 AND c.relname = $2 AND c.relkind IN ('r', 'p') \
           AND a.attnum > 0 AND NOT a.attisdropped",
        &[&schema, &table],
    ).with_context(|| format!("读取目标表 {schema}.{table} 列信息失败"))?;
    if rows.is_empty() {
        bail!("目标表 {schema}.{table} 不存在, 或不是普通表/分区表");
    }
    let mut columns = std::collections::HashMap::with_capacity(rows.len());
    for row in rows {
        let name: String = row.get(0);
        let pg_type: String = row.get(1);
        let generated: bool = row.get(2);
        let identity_always: bool = row.get(3);
        columns.insert(name, (pg_type, generated || identity_always));
    }
    Ok(columns)
}

/// 将数据库目录中的列类型填入映射并拒绝不存在或生成列.
impl Mappings {
    /// 校验所有映射字段并关联其 PostgreSQL 类型.
    fn clone_with_types(
        &self,
        columns: &std::collections::HashMap<String, (String, bool)>,
    ) -> Result<ResolvedMappings> {
        let resolve = |mapping: &ColumnMapping| -> Result<ResolvedColumnMapping> {
            let (pg_type, generated) = columns.get(&mapping.db_column)
                .with_context(|| format!("目标表缺少映射列 {:?}", mapping.db_column))?;
            if *generated {
                bail!("不能写入生成列或 GENERATED ALWAYS identity 列 {:?}", mapping.db_column);
            }
            Ok(ResolvedColumnMapping {
                db_column: mapping.db_column.clone(),
                pg_type: pg_type.clone(),
            })
        };
        Ok(ResolvedMappings {
            primary_keys: self.primary_keys.iter().map(resolve).collect::<Result<_>>()?,
            update_fields: self.update_fields.iter().map(resolve).collect::<Result<_>>()?,
        })
    }
}

/// 生成所有字段均参数化且列类型来自 PostgreSQL 目录的 upsert SQL.
fn build_upsert_sql(schema: &str, table: &str, mappings: &ResolvedMappings) -> String {
    let fields = mappings.primary_keys.iter().chain(&mappings.update_fields).collect::<Vec<_>>();
    let columns = fields.iter().map(|field| quote_identifier(&field.db_column)).collect::<Vec<_>>();
    let values = fields.iter().enumerate()
        .map(|(index, field)| format!("${}::text::{}", index + 1, field.pg_type))
        .collect::<Vec<_>>();
    let conflict = mappings.primary_keys.iter()
        .map(|field| quote_identifier(&field.db_column))
        .collect::<Vec<_>>();
    let update = mappings.update_fields.iter()
        .map(|field| {
            let column = quote_identifier(&field.db_column);
            format!("{column} = EXCLUDED.{column}")
        })
        .collect::<Vec<_>>();
    format!(
        "INSERT INTO {}.{} ({}) VALUES ({}) ON CONFLICT ({}) DO UPDATE SET {}",
        quote_identifier(schema),
        quote_identifier(table),
        columns.join(", "),
        values.join(", "),
        conflict.join(", "),
        update.join(", ")
    )
}
