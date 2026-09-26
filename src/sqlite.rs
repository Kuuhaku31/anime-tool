use anyhow::Result;
use rusqlite::{Connection, params_from_iter};
use std::path::Path;

pub fn write_table(
    db_path: &Path,
    table_name: &str,
    columns: &[String],
    rows: &[indexmap::IndexMap<String, String>],
) -> Result<()> {
    let connection = Connection::open(db_path)?;
    let table = quote_identifier(table_name);
    let quoted_columns: Vec<String> = columns.iter().map(|v| quote_identifier(v)).collect();
    let definitions = quoted_columns
        .iter()
        .map(|v| format!("{v} TEXT"))
        .collect::<Vec<_>>()
        .join(", ");
    connection.execute(&format!("DROP TABLE IF EXISTS {table}"), [])?;
    connection.execute(&format!("CREATE TABLE {table} ({definitions})"), [])?;

    let placeholders = (1..=columns.len())
        .map(|i| format!("?{i}"))
        .collect::<Vec<_>>()
        .join(", ");
    let sql = format!(
        "INSERT INTO {table} ({}) VALUES ({placeholders})",
        quoted_columns.join(", ")
    );

    let mut statement = connection.prepare(&sql)?;
    for row in rows {
        let values = columns
            .iter()
            .map(|column| row.get(column).cloned().unwrap_or_default());
        statement.execute(params_from_iter(values))?;
    }
    Ok(())
}

fn quote_identifier(value: &str) -> String {
    format!("\"{}\"", value.replace('"', "\"\""))
}
