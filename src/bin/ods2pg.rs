

// src/bin/ods2pg.rs
// 按 ODS 配置块或显式列映射逐行 upsert 到 PostgreSQL.

use std::{
    collections::HashSet,
    env,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, bail};
use calamine::{Data, Ods, Reader, open_workbook};
use chrono::{DateTime, FixedOffset, NaiveDate, NaiveDateTime, NaiveTime};
use clap::Parser;
use postgres::{Client, NoTls, Transaction, types::ToSql};

/// 命令行参数和映射模式或配置块模式的剩余参数.
#[derive(Parser)]
#[command(
    name = "ods2pg",
    version,
    about = "从 ODS 文件按配置块或显式映射 upsert PostgreSQL 表",
    override_usage = "ods2pg <ods-file> <sheet-name> <table-name> <primary-map> - <update-map> [OPTIONS]\n       ods2pg <ods-file> <block-name> [<block-name> ...] [--main <entry-sheet>] [OPTIONS]",
    after_help = "显式映射模式需包含独立的 - 分隔符. 配置块模式可使用 --main 指定入口工作表."
)]
struct Cli {
    /// ODS 文件路径.
    ods_file: PathBuf,

    /// 模式参数及可放在任意位置的数据库选项.
    #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
    tail: Vec<String>,
}

/// 解析后的工作模式.
enum RunMode {
    /// 按指定工作表和命令行字段映射导入.
    Explicit {
        sheet_name: String, // ODS 数据工作表名称.
        table: (String, String), // PostgreSQL schema 和表名称.
        mappings: Mappings, // 主键和更新字段映射.
    },
    /// 按入口工作表配置顺序导入多个数据块.
    Blocks {
        main_sheet: String, // 定义导入块的入口工作表名称.
        block_names: Vec<String>, // 按顺序选择的配置块名称.
    },
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

/// 配置块声明的字段类型.
#[derive(Clone, Copy, PartialEq, Eq)]
enum ConfigType {
    /// 普通文本.
    Text,
    /// 有符号 64 位整数.
    Int,
    /// 有限浮点数.
    Float,
    /// 布尔值.
    Bool,
    /// ISO 日期.
    Date,
    /// ISO 时间.
    Time,
    /// 不带时区的 ISO 日期时间.
    DateTime,
    /// 带明确 UTC 偏移的 ISO 日期时间.
    DateTimeTz,
    /// 解码后的 PostgreSQL bytea 二进制值.
    Byte,
}

/// 一个已解析的配置块.
struct ImportBlock {
    name: String,                       // 配置块名称.
    schema: String,                     // PostgreSQL schema 名称.
    table: String,                      // PostgreSQL 表名称.
    sheet: String,                      // 数据工作表名称.
    from_row: usize,                    // 从 1 开始且包含的首行.
    to_row: usize,                      // 从 1 开始但不包含的末行.
    mappings: Mappings,                 // 主键和更新字段映射.
    source_row: usize,                  // 配置块声明行号.
}

/// 正在解析且尚未由 #end_block 结束的配置块.
struct BlockBuilder {
    name: String,                       // 配置块名称.
    source_row: usize,                  // #block 所在行.
    schema: Option<String>,             // 目标 schema.
    table: Option<String>,              // 目标表.
    sheet: Option<String>,              // 数据工作表.
    from_row: Option<usize>,            // 包含的起始行.
    to_row: Option<usize>,              // 不包含的结束行.
    primary_keys: Vec<ColumnMapping>,   // 主键字段.
    update_fields: Vec<ColumnMapping>,  // 冲突时更新的普通字段.
    source_columns: HashSet<usize>,     // 已使用的 ODS 列索引.
    target_columns: HashSet<String>,    // 已使用的数据库列名.
}

/// 配置块及其单元格转换后的待导入数据.
struct PreparedBlock {
    block: ImportBlock,                 // 块定义和目标表.
    rows: Vec<OdsRow>,                  // 已转换的数据行.
    parse_failures: usize,              // ODS 解析或类型转换失败数.
    skipped_empty: usize,               // 全空映射行数.
}

/// 一列 ODS 数据到目标数据库列的映射.
struct ColumnMapping {
    ods_column: usize,    // 从 0 开始的 ODS 列索引.
    db_column: String,    // PostgreSQL 目标列名.
    prefix_hex: bool,     // 非空值是否添加 PostgreSQL 十六进制前缀.
    config_type: Option<ConfigType>, // 配置块声明类型, 旧模式为 None.
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
    binary: bool,         // 是否以 bytea 二进制协议参数写入.
}

/// 经过目标列检查的完整写入映射.
struct ResolvedMappings {
    primary_keys: Vec<ResolvedColumnMapping>,  // 冲突目标列.
    update_fields: Vec<ResolvedColumnMapping>, // 冲突时更新的非主键列.
}

/// 一个已验证 ODS 非空映射行及其原始工作表行号.
struct OdsRow {
    row_number: usize,            // 从 1 开始的工作表行号.
    values: Vec<CellValue>,       // 主键字段后接普通字段的参数值.
}

/// 可绑定到 PostgreSQL 的已转换单元格值.
enum CellValue {
    /// 文本值或 SQL NULL.
    Text(Option<String>),
    /// bytea 二进制值或 SQL NULL.
    Bytes(Option<Vec<u8>>),
}

impl CellValue {
    /// 判断转换后的单元格是否为 SQL NULL.
    fn is_null(&self) -> bool {
        match self {
            Self::Text(value) => value.is_none(),
            Self::Bytes(value) => value.is_none(),
        }
    }

    /// 返回可由 PostgreSQL 客户端绑定的类型化参数.
    fn as_sql_parameter(&self) -> &(dyn ToSql + Sync) {
        match self {
            Self::Text(value) => value,
            Self::Bytes(value) => value,
        }
    }
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
    let (positional, pg_options, main_sheet) = split_cli_options(cli.tail)?;
    let mode = parse_run_mode(positional, main_sheet)?;
    load_dotenv()?;

    match mode {
        RunMode::Explicit { sheet_name, table, mappings } => {
            let (rows, parse_failures, skipped_empty) =
                read_ods_rows(&cli.ods_file, &sheet_name, &mappings)?;
            let mut db = connect_postgres(pg_options)?;
            let (count, write_failures) =
                upsert_rows(&mut db, &table.0, &table.1, &mappings, &rows)?;
            let failures = parse_failures + write_failures;
            println!(
                "从 {} 工作表 {:?} 同步到 {}.{}: 成功 {} 行, 空行 {} 行, 失败 {} 行",
                cli.ods_file.display(), sheet_name, table.0, table.1,
                count, skipped_empty, failures
            );
            if failures > 0 {
                bail!("部分 ODS 数据行导入失败");
            }
        }
        RunMode::Blocks { main_sheet, block_names } => {
            let definitions = parse_config_blocks(&cli.ods_file, &main_sheet)?;
            let selected = select_blocks(definitions, &block_names)?;
            let mut prepared = Vec::with_capacity(selected.len());
            for block in selected {
                let (rows, parse_failures, skipped_empty) = read_block_rows(&cli.ods_file, &block)?;
                prepared.push(PreparedBlock { block, rows, parse_failures, skipped_empty });
            }
            let mut db = connect_postgres(pg_options)?;
            let (mut total_count, mut total_failures, mut total_empty) = (0u64, 0usize, 0usize);
            let mut executed = 0usize;
            for block in &prepared {
                let result = upsert_rows(
                    &mut db, &block.block.schema, &block.block.table,
                    &block.block.mappings, &block.rows,
                );
                let (count, write_failures) = match result {
                    Ok(result) => result,
                    Err(error) => {
                        eprintln!(
                            "配置块 {:?} 在入口工作表第 {} 行执行失败: {error:#}",
                            block.block.name, block.block.source_row
                        );
                        total_failures += 1;
                        break;
                    }
                };
                let failures = block.parse_failures + write_failures;
                total_count += count;
                total_failures += failures;
                total_empty += block.skipped_empty;
                executed += 1;
                println!(
                    "配置块 {:?}: 工作表 {:?}, 表 {}.{}: 成功 {} 行, 空行 {} 行, 失败 {} 行",
                    block.block.name, block.block.sheet, block.block.schema, block.block.table,
                    count, block.skipped_empty, failures
                );
            }
            println!(
                "共执行 {} / {} 个配置块: 成功 {} 行, 空行 {} 行, 失败 {} 行",
                executed, prepared.len(), total_count, total_empty, total_failures
            );
            if total_failures > 0 {
                bail!("部分 ODS 配置块或数据行导入失败");
            }
        }
    }
    Ok(())
}

/// 根据是否存在独立的映射分隔符选择显式映射或配置块模式.
fn parse_run_mode(args: Vec<String>, main_sheet: Option<String>) -> Result<RunMode> {
    if args.iter().any(|arg| arg == "-") {
        if main_sheet.is_some() {
            bail!("--main 只适用于配置块模式");
        }
        if args.len() < 4 {
            bail!("显式映射模式格式为 <sheet> <table> <主键映射> - <普通字段映射>");
        }
        let sheet_name = args[0].clone();
        let table = parse_table_name(&args[1])?;
        let mappings = parse_mappings(&args[2..])?;
        return Ok(RunMode::Explicit { sheet_name, table, mappings });
    }

    if args.is_empty() {
        bail!("配置块模式至少需要一个块名称");
    }
    let mut names = HashSet::new();
    for name in &args {
        if name.is_empty() || name.contains('\0') {
            bail!("配置块名称不能为空或包含 NUL 字符");
        }
        if !names.insert(name.clone()) {
            bail!("配置块名称重复: {name}");
        }
    }
    Ok(RunMode::Blocks {
        main_sheet: main_sheet.unwrap_or_else(|| "main".to_owned()),
        block_names: args,
    })
}

/// 分离数据库选项和入口工作表选项, 保留其余参数的顺序.
fn split_cli_options(args: Vec<String>) -> Result<(Vec<String>, PgOptions, Option<String>)> {
    let mut positional = Vec::new();
    let mut options = PgOptions::default();
    let mut main_sheet = None;
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
        if name == "--main" {
            if main_sheet.is_some() {
                bail!("命令行选项 --main 不能重复指定");
            }
            let value = if let Some(value) = inline_value {
                value.to_owned()
            } else {
                index += 1;
                args.get(index).context("命令行选项 --main 缺少工作表名称")?.clone()
            };
            if value.is_empty() || value.starts_with("--") {
                bail!("--main 必须指定非空的工作表名称");
            }
            main_sheet = Some(value);
        } else if let Some(slot) = slot {
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
    Ok((positional, options, main_sheet))
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
            config_type: None,
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

/// 解析入口工作表中的全部配置块并校验指令结构.
fn parse_config_blocks(path: &Path, sheet_name: &str) -> Result<Vec<ImportBlock>> {
    if !path.is_file() {
        bail!("ODS 路径不是普通文件: {}", path.display());
    }
    let mut workbook: Ods<_> = open_workbook(path)
        .with_context(|| format!("打开 ODS 文件失败: {}", path.display()))?;
    let range = workbook.worksheet_range(sheet_name)
        .with_context(|| format!("读取入口工作表失败: {sheet_name:?}"))?;
    let (_, first_column) = range.start()
        .with_context(|| format!("入口工作表 {sheet_name:?} 没有实际单元格范围"))?;
    let (last_row, last_column) = range.end()
        .with_context(|| format!("入口工作表 {sheet_name:?} 没有实际单元格范围"))?;
    let mut current: Option<BlockBuilder> = None;
    let mut blocks = Vec::new();
    let mut names = HashSet::new();

    for row_index in 0..=last_row {
        let mut cells = (0..=last_column)
            .map(|column_index| {
                range.get_value((row_index, column_index))
                    .map(cell_to_text)
                    .transpose()
                    .map(Option::flatten)
                    .with_context(|| format!(
                        "读取入口工作表第 {} 行第 {} 列失败",
                        row_index + 1, column_index + 1
                    ))
            })
            .collect::<Result<Vec<_>>>()?;
        while cells.last().is_some_and(Option::is_none) {
            cells.pop();
        }
        if cells.is_empty() {
            continue;
        }
        if first_column > 0 && cells[..first_column as usize].iter().any(Option::is_some) {
            bail!("入口工作表第 {} 行指令必须从 A 列开始", row_index + 1);
        }
        let Some(Some(_)) = cells.first() else {
            bail!("入口工作表第 {} 行存在数据但 A 列为空", row_index + 1);
        };
        let mut fields = Vec::with_capacity(cells.len());
        for (index, cell) in cells.into_iter().enumerate() {
            let Some(value) = cell else {
                bail!("入口工作表第 {} 行第 {} 列中间存在空单元格", row_index + 1, index + 1);
            };
            fields.push(value.trim().to_owned());
        }
        let directive = fields[0].as_str();
        match directive {
            "#block" => {
                if let Some(block) = &current {
                    bail!(
                        "入口工作表第 {} 行嵌套定义配置块 {:?}",
                        row_index + 1, block.name
                    );
                }
                require_field_count(&fields, 2, row_index as usize + 1)?;
                let name = fields[1].clone();
                if name.is_empty() || name.contains('\0') {
                    bail!("入口工作表第 {} 行配置块名称无效", row_index + 1);
                }
                if !names.insert(name.clone()) {
                    bail!("入口工作表第 {} 行重复定义配置块 {:?}", row_index + 1, name);
                }
                current = Some(BlockBuilder {
                    name,
                    source_row: row_index as usize + 1,
                    schema: None,
                    table: None,
                    sheet: None,
                    from_row: None,
                    to_row: None,
                    primary_keys: Vec::new(),
                    update_fields: Vec::new(),
                    source_columns: HashSet::new(),
                    target_columns: HashSet::new(),
                });
            }
            "#end_block" => {
                require_field_count(&fields, 1, row_index as usize + 1)?;
                let block = current.take().with_context(|| {
                    format!("入口工作表第 {} 行 #end_block 没有对应的 #block", row_index + 1)
                })?;
                blocks.push(finish_block(block)?);
            }
            "#table" | "#sheet" | "#from" | "#to" => {
                require_field_count(&fields, 2, row_index as usize + 1)?;
                let block = current.as_mut().with_context(|| {
                    format!("入口工作表第 {} 行的 {directive} 必须位于配置块内", row_index + 1)
                })?;
                match directive {
                    "#table" => {
                        if block.table.is_some() {
                            bail!("配置块 {:?} 重复声明 #table", block.name);
                        }
                        let (schema, table) = parse_table_name(&fields[1])
                            .with_context(|| format!("配置块 {:?} 的 #table 无效", block.name))?;
                        block.schema = Some(schema);
                        block.table = Some(table);
                    }
                    "#sheet" => {
                        if block.sheet.replace(fields[1].clone()).is_some() {
                            bail!("配置块 {:?} 重复声明 #sheet", block.name);
                        }
                        if fields[1].is_empty() {
                            bail!("配置块 {:?} 的 #sheet 不能为空", block.name);
                        }
                    }
                    "#from" => {
                        if block.from_row.is_some() {
                            bail!("配置块 {:?} 重复声明 #from", block.name);
                        }
                        block.from_row = Some(parse_config_row_number(&fields[1], row_index as usize + 1)?);
                    }
                    "#to" => {
                        if block.to_row.is_some() {
                            bail!("配置块 {:?} 重复声明 #to", block.name);
                        }
                        block.to_row = Some(parse_config_row_number(&fields[1], row_index as usize + 1)?);
                    }
                    _ => unreachable!(),
                }
            }
            "#pk" | "#k" => {
                require_field_count(&fields, 4, row_index as usize + 1)?;
                let block = current.as_mut().with_context(|| {
                    format!("入口工作表第 {} 行的 {directive} 必须位于配置块内", row_index + 1)
                })?;
                let column = parse_config_column(&fields[3], row_index as usize + 1)?;
                let config_type = parse_config_type(&fields[2])
                    .with_context(|| format!("入口工作表第 {} 行声明类型无效", row_index + 1))?;
                let db_column = fields[1].clone();
                if db_column.is_empty() || db_column.contains('\0') {
                    bail!("入口工作表第 {} 行数据库列名无效", row_index + 1);
                }
                if !block.source_columns.insert(column) {
                    bail!("配置块 {:?} 重复映射 ODS 第 {} 列", block.name, column + 1);
                }
                if !block.target_columns.insert(db_column.clone()) {
                    bail!("配置块 {:?} 重复映射数据库列 {:?}", block.name, db_column);
                }
                let mapping = ColumnMapping {
                    ods_column: column,
                    db_column,
                    prefix_hex: false,
                    config_type: Some(config_type),
                };
                if directive == "#pk" {
                    block.primary_keys.push(mapping);
                } else {
                    block.update_fields.push(mapping);
                }
            }
            _ => bail!("入口工作表第 {} 行存在未知指令 {:?}", row_index + 1, directive),
        }
    }
    if let Some(block) = current {
        bail!("配置块 {:?} 从入口工作表第 {} 行开始但未结束", block.name, block.source_row);
    }
    if blocks.is_empty() {
        bail!("入口工作表 {sheet_name:?} 中没有配置块");
    }
    Ok(blocks)
}

/// 校验指令行的字段数量.
fn require_field_count(fields: &[String], expected: usize, row: usize) -> Result<()> {
    if fields.len() != expected {
        bail!("入口工作表第 {row} 行字段数错误, 期望 {expected} 列, 实际 {} 列", fields.len());
    }
    Ok(())
}

/// 解析从 1 开始的工作表行号.
fn parse_config_row_number(value: &str, row: usize) -> Result<usize> {
    let number = value.parse::<usize>()
        .with_context(|| format!("入口工作表第 {row} 行行号不是正整数: {value:?}"))?;
    if number == 0 {
        bail!("入口工作表第 {row} 行行号必须大于 0");
    }
    Ok(number)
}

/// 解析从 1 开始的十进制源列编号并转换为 0-based 索引.
fn parse_config_column(value: &str, row: usize) -> Result<usize> {
    let number = value.parse::<usize>()
        .with_context(|| format!("入口工作表第 {row} 行列号不是正整数: {value:?}"))?;
    number.checked_sub(1)
        .with_context(|| format!("入口工作表第 {row} 行列号必须大于 0"))
}

/// 将配置中的类型名称转换为受支持的单元格类型.
fn parse_config_type(value: &str) -> Result<ConfigType> {
    match value.to_ascii_lowercase().as_str() {
        "text" => Ok(ConfigType::Text),
        "int" => Ok(ConfigType::Int),
        "float" => Ok(ConfigType::Float),
        "bool" => Ok(ConfigType::Bool),
        "date" => Ok(ConfigType::Date),
        "time" => Ok(ConfigType::Time),
        "datetime" => Ok(ConfigType::DateTime),
        "datetime_tz" => Ok(ConfigType::DateTimeTz),
        "byte" => Ok(ConfigType::Byte),
        _ => bail!("不支持的类型 {value:?}"),
    }
}

/// 检查配置块必填字段并生成不可变定义.
fn finish_block(block: BlockBuilder) -> Result<ImportBlock> {
    let schema = block.schema.with_context(|| {
        format!("配置块 {:?} 从入口工作表第 {} 行开始但缺少 #table", block.name, block.source_row)
    })?;
    let table = block.table.context("配置块缺少目标表")?;
    let sheet = block.sheet.with_context(|| {
        format!("配置块 {:?} 从入口工作表第 {} 行开始但缺少 #sheet", block.name, block.source_row)
    })?;
    let from_row = block.from_row.with_context(|| {
        format!("配置块 {:?} 从入口工作表第 {} 行开始但缺少 #from", block.name, block.source_row)
    })?;
    let to_row = block.to_row.with_context(|| {
        format!("配置块 {:?} 从入口工作表第 {} 行开始但缺少 #to", block.name, block.source_row)
    })?;
    if to_row <= from_row {
        bail!(
            "配置块 {:?} 的 #to 必须大于 #from, 定义位于入口工作表第 {} 行",
            block.name, block.source_row
        );
    }
    if block.primary_keys.is_empty() {
        bail!("配置块 {:?} 从入口工作表第 {} 行开始但缺少 #pk", block.name, block.source_row);
    }
    if block.update_fields.is_empty() {
        bail!("配置块 {:?} 从入口工作表第 {} 行开始但缺少 #k", block.name, block.source_row);
    }
    Ok(ImportBlock {
        name: block.name,
        schema,
        table,
        sheet,
        from_row,
        to_row,
        mappings: Mappings {
            primary_keys: block.primary_keys,
            update_fields: block.update_fields,
        },
        source_row: block.source_row,
    })
}

/// 按命令行指定顺序查找配置块并拒绝未知名称.
fn select_blocks(blocks: Vec<ImportBlock>, requested: &[String]) -> Result<Vec<ImportBlock>> {
    let mut available: std::collections::HashMap<String, ImportBlock> =
        blocks.into_iter().map(|block| (block.name.clone(), block)).collect();
    requested.iter().map(|name| {
        available.remove(name)
            .with_context(|| format!("ODS 配置中未找到配置块 {name:?}"))
    }).collect()
}

/// 读取指定配置块范围, 转换声明类型并保留逐行失败.
fn read_block_rows(path: &Path, block: &ImportBlock) -> Result<(Vec<OdsRow>, usize, usize)> {
    let mut workbook: Ods<_> = open_workbook(path)
        .with_context(|| format!("打开 ODS 文件失败: {}", path.display()))?;
    let range = workbook.worksheet_range(&block.sheet)
        .with_context(|| format!("读取配置块 {:?} 的工作表失败: {:?}", block.name, block.sheet))?;
    let (first_row, first_column) = range.start()
        .with_context(|| format!("配置块 {:?} 的工作表没有实际单元格范围", block.name))?;
    let (last_row, last_column) = range.end()
        .with_context(|| format!("配置块 {:?} 的工作表没有实际单元格范围", block.name))?;
    let start = u32::try_from(block.from_row - 1).context("ODS 起始行超出坐标范围")?;
    let end = u32::try_from(block.to_row - 1).context("ODS 结束行超出坐标范围")?;
    if start < first_row || end == 0 || end - 1 > last_row {
        bail!(
            "配置块 {:?} 的行范围 {}..{} 超出工作表实际范围 {}..={}",
            block.name, block.from_row, block.to_row,
            first_row + 1, last_row + 1
        );
    }
    for mapping in block.mappings.primary_keys.iter().chain(&block.mappings.update_fields) {
        let column = u32::try_from(mapping.ods_column).context("ODS 列索引超出坐标范围")?;
        if column < first_column || column > last_column {
            bail!(
                "配置块 {:?} 映射的 ODS 第 {} 列超出工作表实际列范围 {}..={}",
                block.name, column + 1, first_column + 1, last_column + 1
            );
        }
    }
    let mut rows = Vec::new();
    let (mut failures, mut skipped_empty) = (0, 0);
    for row_index in start..end {
        let values = block.mappings.primary_keys.iter().chain(&block.mappings.update_fields)
            .map(|mapping| {
                let cell = range.get_value((row_index, mapping.ods_column as u32));
                parse_typed_cell(cell, mapping.config_type.expect("配置块映射具有声明类型"))
                    .with_context(|| format!(
                        "配置块 {:?} 的数据工作表 {:?} 第 {} 行第 {} 列转换失败",
                        block.name, block.sheet, row_index + 1, mapping.ods_column + 1
                    ))
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
        if values.iter().all(CellValue::is_null) {
            skipped_empty += 1;
            continue;
        }
        rows.push(OdsRow { row_number: row_index as usize + 1, values });
    }
    Ok((rows, failures, skipped_empty))
}

/// 按配置类型转换单元格并保留 SQL NULL 与 bytea 二进制值.
fn parse_typed_cell(cell: Option<&Data>, config_type: ConfigType) -> Result<CellValue> {
    let Some(cell) = cell.filter(|cell| !matches!(cell, Data::Empty)) else {
        return Ok(match config_type {
            ConfigType::Byte => CellValue::Bytes(None),
            _ => CellValue::Text(None),
        });
    };
    if config_type == ConfigType::Byte {
        let value = match cell {
            Data::String(value) => value,
            _ => bail!("byte 类型只接受十六进制字符串"),
        };
        if value.len() % 2 != 0 || !value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            bail!("byte 类型必须是偶数位 ASCII 十六进制字符串, 且不带前缀或空白");
        }
        return Ok(CellValue::Bytes(Some(hex::decode(value)?)));
    }
    let text = cell_to_text(cell)?.unwrap_or_default();
    let trimmed = text.trim();
    let value = match config_type {
        ConfigType::Text => text,
        ConfigType::Int => parse_i64_cell(cell, trimmed)?.to_string(),
        ConfigType::Float => {
            let value = numeric_cell(cell, trimmed)?;
            if !value.is_finite() {
                bail!("float 类型不接受非有限数字");
            }
            value.to_string()
        }
        ConfigType::Bool => parse_bool_cell(cell, trimmed)?.to_string(),
        ConfigType::Date => parse_date_cell(cell, trimmed)?.format("%Y-%m-%d").to_string(),
        ConfigType::Time => parse_time_cell(cell, trimmed)?.format("%H:%M:%S%.f").to_string(),
        ConfigType::DateTime => parse_datetime_cell(cell, trimmed)?.format("%Y-%m-%dT%H:%M:%S%.f").to_string(),
        ConfigType::DateTimeTz => parse_datetime_tz_cell(trimmed)?.to_rfc3339(),
        ConfigType::Byte => unreachable!(),
    };
    Ok(CellValue::Text(Some(value)))
}

/// 将 ODS 数字或整数文本转换为有限数字.
fn numeric_cell(cell: &Data, text: &str) -> Result<f64> {
    match cell {
        Data::Int(value) => Ok(*value as f64),
        Data::Float(value) => Ok(*value),
        _ => text.parse::<f64>().context("不是有效数字"),
    }
}

/// 将 ODS 数值或整数文本转换为有符号 64 位整数.
fn parse_i64_cell(cell: &Data, text: &str) -> Result<i64> {
    match cell {
        Data::Int(value) => i64::try_from(*value).context("整数超出 PostgreSQL bigint 范围"),
        Data::Float(value) => {
            if !value.is_finite() || value.fract() != 0.0
                || *value < i64::MIN as f64 || *value >= 9223372036854775808.0
            {
                bail!("不是 PostgreSQL bigint 范围内的整数");
            }
            Ok(*value as i64)
        }
        _ => text.parse::<i64>().context("不是 PostgreSQL bigint 范围内的整数"),
    }
}

/// 将 ODS 布尔值或受支持文本转换为标准布尔字面值.
fn parse_bool_cell(cell: &Data, text: &str) -> Result<bool> {
    match cell {
        Data::Bool(value) => Ok(*value),
        _ => match text.to_ascii_lowercase().as_str() {
            "true" | "1" => Ok(true),
            "false" | "0" => Ok(false),
            _ => bail!("bool 类型只接受 true, false, 1 或 0"),
        },
    }
}

/// 解析日期对象或 ISO 日期文本.
fn parse_date_cell(cell: &Data, text: &str) -> Result<NaiveDate> {
    if let Data::DateTime(value) = cell {
        return value.as_datetime().map(|value| value.date())
            .context("ODS 日期数值无法转换为日期");
    }
    NaiveDate::parse_from_str(text, "%Y-%m-%d").context("date 类型需要 YYYY-MM-DD 日期")
}

/// 解析 ODS 时间对象或 ISO 时间文本.
fn parse_time_cell(cell: &Data, text: &str) -> Result<NaiveTime> {
    if let Data::DateTime(value) = cell {
        return value.as_datetime().map(|value| value.time())
            .context("ODS 时间数值无法转换为时间");
    }
    for format in ["%H:%M:%S%.f", "%H:%M:%S", "%H:%M"] {
        if let Ok(value) = NaiveTime::parse_from_str(text, format) {
            return Ok(value);
        }
    }
    bail!("time 类型需要 ISO 时间")
}

/// 解析无时区日期时间对象或 ISO 日期时间文本.
fn parse_datetime_cell(cell: &Data, text: &str) -> Result<NaiveDateTime> {
    if let Data::DateTime(value) = cell {
        return value.as_datetime().context("ODS 日期时间数值无法转换");
    }
    for format in ["%Y-%m-%dT%H:%M:%S%.f", "%Y-%m-%d %H:%M:%S%.f"] {
        if let Ok(value) = NaiveDateTime::parse_from_str(text, format) {
            return Ok(value);
        }
    }
    bail!("datetime 类型需要不带时区的 ISO 日期时间")
}

/// 仅解析含 Z 或明确 UTC 偏移的 RFC 3339 日期时间.
fn parse_datetime_tz_cell(text: &str) -> Result<DateTime<FixedOffset>> {
    if !text.ends_with('Z') && !text.contains('+')
        && !text.rfind('-').is_some_and(|index| index > 10)
    {
        bail!("datetime_tz 必须包含 Z 或明确的 UTC 偏移");
    }
    DateTime::parse_from_rfc3339(text).context("datetime_tz 需要有效的 RFC 3339 日期时间")
}

/// 打开指定 ODS 工作表并自动遍历 calamine 返回的实际使用范围.
fn read_ods_rows(
    path: &Path,
    sheet_name: &str,
    mappings: &Mappings,
) -> Result<(Vec<OdsRow>, usize, usize)> {
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
    let mut skipped_empty = 0;
    for row_index in first_row..=last_row {
        let values = mappings.primary_keys.iter().chain(&mappings.update_fields)
            .map(|mapping| {
                let column_index = u32::try_from(mapping.ods_column).context("ODS 列索引超出坐标范围")?;
                let cell = range.get_value((row_index, column_index))
                    .with_context(|| format!("ODS 第 {} 行第 {} 列超出工作表范围", row_index + 1, column_index + 1))?;
                let value = cell_to_text(cell)
                    .with_context(|| format!("读取 ODS 第 {} 行第 {} 列失败", row_index + 1, column_index + 1))?;
                Ok(match value {
                    Some(value) => {
                        let value = if mapping.prefix_hex { format!("\\x{value}") } else { value };
                        CellValue::Text(Some(value))
                    }
                    None => CellValue::Text(None),
                })
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
        if values.iter().all(CellValue::is_null) {
            skipped_empty += 1;
            continue;
        }
        rows.push(OdsRow {
            row_number: row_index as usize + 1,
            values,
        });
    }
    Ok((rows, failures, skipped_empty))
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
    let update_sql = build_update_sql(schema, table, &resolved);
    transaction.commit().context("提交目标表检查事务失败")?;
    let statement = db.prepare(&sql).context("准备 upsert SQL 失败")?;
    let update_statement = db.prepare(&update_sql).context("准备按主键更新 SQL 失败")?;
    let mut total = 0u64;
    let mut failures = 0;
    for row in rows {
        if mappings.primary_keys.iter().enumerate().any(|(index, _)| row.values[index].is_null()) {
            eprintln!("ODS 第 {} 行的主键映射不能为空", row.row_number);
            failures += 1;
            continue;
        }
        let params = row.values.iter().map(CellValue::as_sql_parameter).collect::<Vec<_>>();
        let mut row_transaction = db.transaction().context("开始 ODS 数据行事务失败")?;
        // 先更新现有记录, 避免 INSERT 在冲突处理前检查未映射的必填列.
        // 未匹配时才插入, 并保留 ON CONFLICT 处理并发插入产生的主键冲突.
        let write_result = row_transaction.execute(&update_statement, &params)
            .and_then(|count| {
                if count == 0 {
                    row_transaction.execute(&statement, &params)
                } else {
                    Ok(count)
                }
            });
        let result = match write_result {
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
            let binary = mapping.config_type == Some(ConfigType::Byte);
            if binary && pg_type != "bytea" {
                bail!("配置块的 byte 类型只能映射到 PostgreSQL bytea 列 {:?}", mapping.db_column);
            }
            Ok(ResolvedColumnMapping {
                db_column: mapping.db_column.clone(),
                pg_type: pg_type.clone(),
                binary,
            })
        };
        Ok(ResolvedMappings {
            primary_keys: self.primary_keys.iter().map(resolve).collect::<Result<_>>()?,
            update_fields: self.update_fields.iter().map(resolve).collect::<Result<_>>()?,
        })
    }
}

/// 按列的真实类型生成参数表达式, 两种写入路径保持相同的参数类型.
impl ResolvedColumnMapping {
    /// 根据从 1 开始的参数编号生成安全的 SQL 类型转换.
    fn parameter_sql(&self, index: usize) -> String {
        if self.binary {
            format!("${index}::bytea")
        } else {
            format!("${index}::text::{}", self.pg_type)
        }
    }
}

/// 生成只修改映射字段的 UPDATE, 主键参数编号与 INSERT 保持一致.
fn build_update_sql(schema: &str, table: &str, mappings: &ResolvedMappings) -> String {
    let conditions = mappings.primary_keys.iter().enumerate()
        .map(|(index, field)| {
            format!("{} = {}", quote_identifier(&field.db_column), field.parameter_sql(index + 1))
        })
        .collect::<Vec<_>>();
    let updates = mappings.update_fields.iter().enumerate()
        .map(|(index, field)| {
            format!(
                "{} = {}", quote_identifier(&field.db_column),
                field.parameter_sql(mappings.primary_keys.len() + index + 1)
            )
        })
        .collect::<Vec<_>>();
    format!(
        "UPDATE {}.{} SET {} WHERE {}",
        quote_identifier(schema), quote_identifier(table),
        updates.join(", "), conditions.join(" AND ")
    )
}

/// 生成所有字段均参数化且列类型来自 PostgreSQL 目录的 upsert SQL.
fn build_upsert_sql(schema: &str, table: &str, mappings: &ResolvedMappings) -> String {
    let fields = mappings.primary_keys.iter().chain(&mappings.update_fields).collect::<Vec<_>>();
    let columns = fields.iter().map(|field| quote_identifier(&field.db_column)).collect::<Vec<_>>();
    let values = fields.iter().enumerate()
        .map(|(index, field)| field.parameter_sql(index + 1))
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
