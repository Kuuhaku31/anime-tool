use anime_tool::{markdown, sqlite};
use anyhow::{Context, Result, anyhow};
use clap::Parser;
use indexmap::IndexMap;
use std::path::PathBuf;

#[derive(Debug, Parser)]
#[command(about = "解析 Markdown 标题和 meta, 输出 TSV CSV, 可选导入 SQLite")]
struct Args {
    /// 输入 Markdown 文件
    input: PathBuf,
    /// 输出 TSV CSV 文件, 不指定则不输出文件
    #[arg(short, long)]
    output: Option<PathBuf>,
    /// 导入 SQLite, 格式为 <sqlite_db_path>:<table_name>
    #[arg(long, value_name = "SQLITE_DB:TABLE")]
    to_db: Option<String>,
}

fn parse_db_target(value: &str) -> Result<(PathBuf, String)> {
    let (path, table) = value
        .rsplit_once(':')
        .ok_or_else(|| anyhow!("--to-db 必须使用 <sqlite_db_path>:<table_name> 格式"))?;
    if path.is_empty() || table.is_empty() {
        return Err(anyhow!("SQLite 路径和表名不能为空"));
    }
    Ok((PathBuf::from(path), table.to_string()))
}

fn parse_file(path: &PathBuf) -> Result<(Vec<String>, Vec<IndexMap<String, String>>)> {
    let text = std::fs::read_to_string(path)
        .with_context(|| format!("读取 Markdown 失败: {}", path.display()))?;
    let lines: Vec<String> = text.lines().map(str::to_string).collect();
    let sections = markdown::split_sections(&lines);
    let mut rows = Vec::new();

    for section in sections {
        let mut row = IndexMap::new();
        let title = section.title.clone().unwrap_or_else(|| {
            path.file_name()
                .and_then(|v| v.to_str())
                .unwrap_or_default()
                .to_string()
        });
        row.insert("title".to_string(), title);

        if let Some((start, end)) = markdown::find_meta_blocks(&lines, section.start, section.end)
            .first()
            .copied()
        {
            let meta = markdown::parse_meta(&lines[start + 1..end]);
            for (key, value) in meta {
                row.insert(key, value);
            }
        }
        rows.push(row);
    }

    if rows.is_empty() {
        let mut row = IndexMap::new();
        row.insert(
            "title".to_string(),
            path.file_name()
                .and_then(|v| v.to_str())
                .unwrap_or_default()
                .to_string(),
        );
        rows.push(row);
    }

    let mut columns = vec!["title".to_string()];
    for row in &rows {
        for key in row.keys() {
            if key != "title" && !columns.iter().any(|v| v == key) {
                columns.push(key.clone());
            }
        }
    }
    Ok((columns, rows))
}

fn write_tsv(path: &PathBuf, columns: &[String], rows: &[IndexMap<String, String>]) -> Result<()> {
    use std::fs::File;
    use std::io::Write;

    let file = File::create(path).with_context(|| format!("创建 TSV 失败: {}", path.display()))?;
    let mut file = std::io::BufWriter::new(file);

    // 与原 Python 脚本保持一致, 使用 UTF-8 BOM 方便 Excel/Calc 识别中文.
    file.write_all(&[0xEF, 0xBB, 0xBF])?;

    let mut writer = csv::WriterBuilder::new()
        .delimiter(b'\t')
        .has_headers(false)
        .from_writer(file);

    writer.write_record(columns)?;
    for row in rows {
        let values = columns
            .iter()
            .map(|key| row.get(key).map(String::as_str).unwrap_or(""));
        writer.write_record(values)?;
    }

    writer.flush()?;
    Ok(())
}

fn main() -> Result<()> {
    let args = Args::parse();
    if !args.input.is_file() {
        return Err(anyhow!("输入文件不存在: {}", args.input.display()));
    }
    let (columns, rows) = parse_file(&args.input)?;
    println!("Input:   {}", args.input.display());
    println!("Rows:    {}", rows.len());
    println!("Columns: {}", columns.len());

    if let Some(output) = args.output {
        write_tsv(&output, &columns, &rows)?;
        println!("结果已写入 CSV 文件: {}", output.display());
    }

    if let Some(to_db) = args.to_db {
        let (db_path, table) = parse_db_target(&to_db)?;
        sqlite::write_table(&db_path, &table, &columns, &rows)?;
        println!(
            "结果已写入 SQLite 数据库: {}, 表: {}",
            db_path.display(),
            table
        );
    }
    Ok(())
}
