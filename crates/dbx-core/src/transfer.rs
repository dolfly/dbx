use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use tokio::sync::RwLock;

use crate::connection::{AppState, PoolKind};
use crate::db;
use crate::models::connection::DatabaseType;
use crate::query::{agent_execute_query_params, QueryExecutionOptions};

static CANCELLED: std::sync::LazyLock<RwLock<HashSet<String>>> =
    std::sync::LazyLock::new(|| RwLock::new(HashSet::new()));

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
#[serde(rename_all = "camelCase")]
pub enum TransferMode {
    #[default]
    Append,
    Overwrite,
    Upsert,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TransferRequest {
    pub transfer_id: String,
    pub source_connection_id: String,
    pub source_database: String,
    pub source_schema: String,
    pub target_connection_id: String,
    pub target_database: String,
    pub target_schema: String,
    pub tables: Vec<String>,
    pub create_table: bool,
    #[serde(default)]
    pub mode: TransferMode,
    pub batch_size: usize,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TransferProgress {
    pub transfer_id: String,
    pub table: String,
    pub table_index: usize,
    pub total_tables: usize,
    pub rows_transferred: u64,
    pub total_rows: Option<u64>,
    pub status: TransferStatus,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum TransferStatus {
    Running,
    TableDone,
    Done,
    Error,
    Cancelled,
}

pub fn quote_identifier(name: &str, db_type: &DatabaseType) -> String {
    match db_type {
        DatabaseType::Mysql | DatabaseType::ClickHouse | DatabaseType::Doris | DatabaseType::StarRocks => {
            format!("`{}`", name.replace('`', "``"))
        }
        DatabaseType::SqlServer => format!("[{}]", name.replace(']', "]]")),
        _ => format!("\"{}\"", name.replace('"', "\"\"")),
    }
}

pub fn qualified_table(table: &str, schema: &str, db_type: &DatabaseType) -> String {
    let qt = quote_identifier(table, db_type);
    if schema.is_empty() {
        qt
    } else {
        format!("{}.{}", quote_identifier(schema, db_type), qt)
    }
}

pub fn escape_value(val: &serde_json::Value, db_type: &DatabaseType) -> String {
    escape_value_typed(val, db_type, None)
}

pub fn escape_value_typed(val: &serde_json::Value, db_type: &DatabaseType, column_type: Option<&str>) -> String {
    match val {
        serde_json::Value::Null => "NULL".to_string(),
        serde_json::Value::Bool(b) => match db_type {
            DatabaseType::Mysql
            | DatabaseType::Sqlite
            | DatabaseType::DuckDb
            | DatabaseType::Doris
            | DatabaseType::StarRocks => {
                if *b {
                    "1".to_string()
                } else {
                    "0".to_string()
                }
            }
            _ => {
                if *b {
                    "TRUE".to_string()
                } else {
                    "FALSE".to_string()
                }
            }
        },
        serde_json::Value::Number(n) => n.to_string(),
        serde_json::Value::String(s) => {
            format!("'{}'", format_literal_string(s, db_type, column_type).replace('\\', "\\\\").replace('\'', "''"))
        }
        serde_json::Value::Array(arr) => format_pg_array_sql_literal(arr),
        _ => {
            let s = val.to_string();
            format!("'{}'", s.replace('\\', "\\\\").replace('\'', "''"))
        }
    }
}

pub fn format_pg_array_sql_literal(arr: &[serde_json::Value]) -> String {
    if arr.is_empty() {
        return "'{}'".to_string();
    }
    let elements: Vec<String> = arr.iter().map(format_pg_array_element).collect();
    let inner = format!("{{{}}}", elements.join(","));
    format!("'{}'", inner.replace('\\', "\\\\").replace('\'', "''"))
}

fn format_pg_array_element(val: &serde_json::Value) -> String {
    match val {
        serde_json::Value::Null => "NULL".to_string(),
        serde_json::Value::Array(arr) => {
            if arr.is_empty() {
                return "{}".to_string();
            }
            let elements: Vec<String> = arr.iter().map(format_pg_array_element).collect();
            format!("{{{}}}", elements.join(","))
        }
        serde_json::Value::String(s) => {
            let escaped = s.replace('\\', "\\\\").replace('"', "\\\"");
            format!("\"{}\"", escaped)
        }
        serde_json::Value::Number(n) => n.to_string(),
        serde_json::Value::Bool(b) => {
            if *b {
                "true".to_string()
            } else {
                "false".to_string()
            }
        }
        serde_json::Value::Object(o) => {
            let json = serde_json::to_string(o).unwrap_or_default();
            let escaped = json.replace('\\', "\\\\").replace('"', "\\\"");
            format!("\"{}\"", escaped)
        }
    }
}

fn format_literal_string(value: &str, db_type: &DatabaseType, column_type: Option<&str>) -> String {
    if is_mysql_datetime_literal_database(db_type) && column_type.map(is_temporal_column_type).unwrap_or(true) {
        normalize_mysql_temporal_literal(value, column_type).unwrap_or_else(|| value.to_string())
    } else {
        value.to_string()
    }
}

fn is_mysql_datetime_literal_database(db_type: &DatabaseType) -> bool {
    matches!(
        db_type,
        DatabaseType::Mysql
            | DatabaseType::Doris
            | DatabaseType::StarRocks
            | DatabaseType::Goldendb
            | DatabaseType::Sundb
    )
}

fn normalize_mysql_temporal_literal(value: &str, column_type: Option<&str>) -> Option<String> {
    let bytes = value.as_bytes();
    if bytes.len() < 20 || !is_mysql_datetime_base(bytes) {
        return None;
    }

    let rest = &value[19..];
    let (fraction, offset) = if let Some(after_dot) = rest.strip_prefix('.') {
        let digit_count = after_dot.bytes().take_while(|b| b.is_ascii_digit()).count();
        if digit_count == 0 {
            return None;
        }
        let fraction_len = 1 + digit_count;
        (&rest[..fraction_len.min(7)], &rest[fraction_len..])
    } else {
        ("", rest)
    };

    if !is_timezone_suffix(offset) {
        return None;
    }

    match temporal_column_kind(column_type) {
        Some("date") => Some(value[..10].to_string()),
        Some("time") => Some(format!("{}{}", &value[11..19], fraction)),
        _ => Some(format!("{} {}{}", &value[..10], &value[11..19], fraction)),
    }
}

fn is_temporal_column_type(column_type: &str) -> bool {
    temporal_column_kind(Some(column_type)).is_some()
}

fn temporal_column_kind(column_type: Option<&str>) -> Option<&'static str> {
    let base = column_type?.trim().to_ascii_lowercase();
    let base = base.split(['(', ':', ' ']).next().unwrap_or("");
    match base {
        "date" => Some("date"),
        "time" => Some("time"),
        "datetime" | "timestamp" => Some("datetime"),
        _ => None,
    }
}

fn is_mysql_datetime_base(bytes: &[u8]) -> bool {
    matches!(
        bytes,
        [
            y0,
            y1,
            y2,
            y3,
            b'-',
            m0,
            m1,
            b'-',
            d0,
            d1,
            sep,
            h0,
            h1,
            b':',
            min0,
            min1,
            b':',
            s0,
            s1,
            ..
        ] if y0.is_ascii_digit()
            && y1.is_ascii_digit()
            && y2.is_ascii_digit()
            && y3.is_ascii_digit()
            && m0.is_ascii_digit()
            && m1.is_ascii_digit()
            && d0.is_ascii_digit()
            && d1.is_ascii_digit()
            && (*sep == b'T' || *sep == b' ')
            && h0.is_ascii_digit()
            && h1.is_ascii_digit()
            && min0.is_ascii_digit()
            && min1.is_ascii_digit()
            && s0.is_ascii_digit()
            && s1.is_ascii_digit()
    )
}

fn is_timezone_suffix(value: &str) -> bool {
    if value.eq_ignore_ascii_case("z") {
        return true;
    }
    let bytes = value.as_bytes();
    matches!(
        bytes,
        [sign, h0, h1, b':', m0, m1]
            if (*sign == b'+' || *sign == b'-')
                && h0.is_ascii_digit()
                && h1.is_ascii_digit()
                && m0.is_ascii_digit()
                && m1.is_ascii_digit()
    )
}

pub fn map_column_type(source_type: &str, _source_db: &DatabaseType, target_db: &DatabaseType) -> String {
    let t = source_type.to_lowercase();
    let base = t.split('(').next().unwrap_or(&t).trim();

    match base {
        "int" | "integer" | "int4" | "mediumint" => match target_db {
            DatabaseType::Postgres => "INTEGER".into(),
            DatabaseType::Mysql => "INT".into(),
            DatabaseType::SqlServer => "INT".into(),
            _ => "INTEGER".into(),
        },
        "bigint" | "int8" => "BIGINT".into(),
        "smallint" | "int2" => "SMALLINT".into(),
        "tinyint" => match target_db {
            DatabaseType::Postgres => "SMALLINT".into(),
            _ => "TINYINT".into(),
        },
        "serial" | "bigserial" | "smallserial" => match target_db {
            DatabaseType::Postgres => source_type.to_uppercase(),
            DatabaseType::Mysql => "BIGINT AUTO_INCREMENT".into(),
            _ => "INTEGER".into(),
        },
        "float" | "float4" | "real" => match target_db {
            DatabaseType::Postgres => "REAL".into(),
            _ => "FLOAT".into(),
        },
        "double" | "double precision" | "float8" => match target_db {
            DatabaseType::Postgres => "DOUBLE PRECISION".into(),
            _ => "DOUBLE".into(),
        },
        "decimal" | "numeric" | "number" => {
            if t.contains('(') {
                match target_db {
                    DatabaseType::Mysql | DatabaseType::Postgres | DatabaseType::SqlServer | DatabaseType::Oracle => {
                        format!("DECIMAL{}", &t[t.find('(').unwrap()..])
                    }
                    _ => "NUMERIC".into(),
                }
            } else {
                "NUMERIC".into()
            }
        }
        "varchar" | "nvarchar" | "character varying" | "varchar2" => {
            if t.contains('(') {
                let len_part = &t[t.find('(').unwrap()..];
                match target_db {
                    DatabaseType::Postgres => format!("VARCHAR{len_part}"),
                    DatabaseType::Mysql => format!("VARCHAR{len_part}"),
                    DatabaseType::SqlServer => format!("NVARCHAR{len_part}"),
                    _ => format!("VARCHAR{len_part}"),
                }
            } else {
                "VARCHAR(255)".into()
            }
        }
        "char" | "nchar" | "character" => {
            if t.contains('(') {
                let len_part = &t[t.find('(').unwrap()..];
                format!("CHAR{len_part}")
            } else {
                "CHAR(1)".into()
            }
        }
        "text" | "longtext" | "mediumtext" | "tinytext" | "clob" | "ntext" => "TEXT".into(),
        "bool" | "boolean" => match target_db {
            DatabaseType::Mysql => "TINYINT(1)".into(),
            DatabaseType::SqlServer => "BIT".into(),
            _ => "BOOLEAN".into(),
        },
        "date" => "DATE".into(),
        "time" => "TIME".into(),
        "datetime" => match target_db {
            DatabaseType::Postgres => "TIMESTAMP".into(),
            _ => "DATETIME".into(),
        },
        "timestamp" | "timestamptz" | "timestamp with time zone" | "timestamp without time zone" => match target_db {
            DatabaseType::Mysql => "DATETIME".into(),
            DatabaseType::SqlServer => "DATETIME2".into(),
            _ => "TIMESTAMP".into(),
        },
        "blob" | "longblob" | "mediumblob" | "tinyblob" | "binary" | "varbinary" | "image" => match target_db {
            DatabaseType::Postgres => "BYTEA".into(),
            DatabaseType::Mysql => "BLOB".into(),
            DatabaseType::SqlServer => "VARBINARY(MAX)".into(),
            _ => "BLOB".into(),
        },
        "bytea" => match target_db {
            DatabaseType::Postgres => "BYTEA".into(),
            DatabaseType::Mysql => "BLOB".into(),
            _ => "BLOB".into(),
        },
        "json" | "jsonb" => match target_db {
            DatabaseType::Postgres => "JSONB".into(),
            DatabaseType::Mysql => "JSON".into(),
            _ => "TEXT".into(),
        },
        "uuid" => match target_db {
            DatabaseType::Postgres => "UUID".into(),
            _ => "VARCHAR(36)".into(),
        },
        "bit" => match target_db {
            DatabaseType::Postgres => "BOOLEAN".into(),
            _ => "BIT".into(),
        },
        _ => "TEXT".into(),
    }
}

fn mysql_type_needs_key_prefix(mapped_type: &str) -> bool {
    let base = mapped_type.split('(').next().unwrap_or(mapped_type).trim().to_ascii_lowercase();
    matches!(
        base.as_str(),
        "text" | "tinytext" | "mediumtext" | "longtext" | "blob" | "tinyblob" | "mediumblob" | "longblob"
    )
}

pub fn generate_create_table_ddl(
    columns: &[db::ColumnInfo],
    table: &str,
    schema: &str,
    target_db: &DatabaseType,
    source_db: &DatabaseType,
    table_comment: Option<&str>,
) -> String {
    let full_table = qualified_table(table, schema, target_db);

    let is_mysql_family = matches!(
        target_db,
        DatabaseType::Mysql
            | DatabaseType::Doris
            | DatabaseType::StarRocks
            | DatabaseType::Goldendb
            | DatabaseType::Sundb
    );

    let mut col_lines = Vec::with_capacity(columns.len());
    for c in columns {
        col_lines.push({
            let mapped_type = map_column_type(&c.data_type, source_db, target_db);
            let mut line = format!("  {} {}", quote_identifier(&c.name, target_db), mapped_type);
            if !c.is_nullable {
                line.push_str(" NOT NULL");
            }
            if is_mysql_family {
                if let Some(ref comment) = c.comment {
                    let trimmed = comment.trim();
                    if !trimmed.is_empty() {
                        line.push_str(&format!(" COMMENT '{}'", trimmed.replace('\'', "''")));
                    }
                }
            }
            line
        });
    }

    let mut pks = Vec::new();
    pks.reserve(columns.iter().filter(|c| c.is_primary_key).count());
    for c in columns {
        if c.is_primary_key {
            let qname = quote_identifier(&c.name, target_db);
            if is_mysql_family {
                let mapped = map_column_type(&c.data_type, source_db, target_db);
                if mysql_type_needs_key_prefix(&mapped) {
                    pks.push(format!("{qname}(255)"));
                    continue;
                }
            }
            pks.push(qname);
        }
    }

    let mut ddl = match target_db {
        DatabaseType::SqlServer => {
            format!("IF NOT EXISTS (SELECT * FROM INFORMATION_SCHEMA.TABLES WHERE TABLE_NAME = '{table}')\n")
        }
        _ => String::new(),
    };

    let create_prefix = match target_db {
        DatabaseType::SqlServer => "CREATE TABLE",
        _ => "CREATE TABLE IF NOT EXISTS",
    };

    ddl.push_str(&format!("{create_prefix} {full_table} (\n"));
    ddl.push_str(&col_lines.join(",\n"));

    if !pks.is_empty() {
        ddl.push_str(&format!(",\n  PRIMARY KEY ({})", pks.join(", ")));
    }

    ddl.push_str("\n)");

    if is_mysql_family {
        if let Some(ref comment) = table_comment {
            let trimmed = comment.trim();
            if !trimmed.is_empty() {
                ddl.push_str(&format!(" COMMENT='{}'", trimmed.replace('\'', "''")));
            }
        }
    }

    if matches!(target_db, DatabaseType::ClickHouse) {
        ddl.push_str(" ENGINE = MergeTree() ORDER BY tuple()");
    }

    ddl
}

/// Generate COMMENT ON COLUMN / ALTER TABLE COMMENT COLUMN / COMMENT ON TABLE
/// statements for databases that don't support inline comments in CREATE TABLE.
/// MySQL family uses inline syntax (handled in generate_create_table_ddl).
pub fn generate_comment_ddl(
    columns: &[db::ColumnInfo],
    table: &str,
    schema: &str,
    target_db: &DatabaseType,
    table_comment: Option<&str>,
) -> Vec<String> {
    if !matches!(target_db, DatabaseType::Postgres | DatabaseType::Oracle | DatabaseType::ClickHouse) {
        return Vec::new();
    }

    let full_table = qualified_table(table, schema, target_db);
    let mut statements = Vec::new();

    // Table-level comment first (PostgreSQL/Oracle only; ClickHouse doesn't support COMMENT ON TABLE)
    if matches!(target_db, DatabaseType::Postgres | DatabaseType::Oracle) {
        if let Some(ref comment) = table_comment {
            let trimmed = comment.trim();
            if !trimmed.is_empty() {
                let escaped = trimmed.replace('\'', "''");
                statements.push(format!("COMMENT ON TABLE {full_table} IS '{escaped}'"));
            }
        }
    }

    for c in columns {
        if let Some(ref comment) = c.comment {
            let trimmed = comment.trim();
            if trimmed.is_empty() {
                continue;
            }
            let escaped = trimmed.replace('\'', "''");
            let qcol = quote_identifier(&c.name, target_db);

            match target_db {
                DatabaseType::Postgres | DatabaseType::Oracle => {
                    statements.push(format!("COMMENT ON COLUMN {full_table}.{qcol} IS '{escaped}'"));
                }
                DatabaseType::ClickHouse => {
                    statements.push(format!("ALTER TABLE {full_table} COMMENT COLUMN {qcol} '{escaped}'"));
                }
                _ => {}
            }
        }
    }

    statements
}

pub fn generate_insert(
    columns: &[String],
    rows: &[Vec<serde_json::Value>],
    table: &str,
    schema: &str,
    db_type: &DatabaseType,
) -> String {
    generate_insert_typed(columns, &vec![None; columns.len()], rows, table, schema, db_type)
}

pub fn generate_insert_typed(
    columns: &[String],
    column_types: &[Option<String>],
    rows: &[Vec<serde_json::Value>],
    table: &str,
    schema: &str,
    db_type: &DatabaseType,
) -> String {
    if rows.is_empty() {
        return String::new();
    }

    let full_table = qualified_table(table, schema, db_type);
    let col_list = columns.iter().map(|c| quote_identifier(c, db_type)).collect::<Vec<_>>().join(", ");

    let value_rows = value_rows_sql(rows, column_types, db_type);

    format!("INSERT INTO {full_table} ({col_list}) VALUES\n{}", value_rows.join(",\n"))
}

fn value_rows_sql(
    rows: &[Vec<serde_json::Value>],
    column_types: &[Option<String>],
    db_type: &DatabaseType,
) -> Vec<String> {
    let mut out = Vec::with_capacity(rows.len());
    for row in rows {
        let mut vals = Vec::with_capacity(row.len());
        for (index, v) in row.iter().enumerate() {
            vals.push(escape_value_typed(v, db_type, column_types.get(index).and_then(|value| value.as_deref())));
        }
        out.push(format!("({})", vals.join(", ")));
    }
    out
}

pub fn generate_upsert(
    columns: &[String],
    rows: &[Vec<serde_json::Value>],
    table: &str,
    schema: &str,
    db_type: &DatabaseType,
    pk_columns: &[String],
) -> String {
    generate_upsert_typed(columns, &vec![None; columns.len()], rows, table, schema, db_type, pk_columns)
}

pub fn generate_upsert_typed(
    columns: &[String],
    column_types: &[Option<String>],
    rows: &[Vec<serde_json::Value>],
    table: &str,
    schema: &str,
    db_type: &DatabaseType,
    pk_columns: &[String],
) -> String {
    if rows.is_empty() || pk_columns.is_empty() {
        return String::new();
    }

    let full_table = qualified_table(table, schema, db_type);
    let col_list = columns.iter().map(|c| quote_identifier(c, db_type)).collect::<Vec<_>>().join(", ");

    let value_rows = value_rows_sql(rows, column_types, db_type);

    let mut non_pk_columns = Vec::with_capacity(columns.len().saturating_sub(pk_columns.len()));
    for c in columns {
        if !pk_columns.contains(c) {
            non_pk_columns.push(c);
        }
    }

    match db_type {
        DatabaseType::Postgres | DatabaseType::Sqlite | DatabaseType::DuckDb => {
            let pk_list = pk_columns.iter().map(|c| quote_identifier(c, db_type)).collect::<Vec<_>>().join(", ");
            let mut sql = format!("INSERT INTO {full_table} ({col_list}) VALUES\n{}", value_rows.join(",\n"));
            if non_pk_columns.is_empty() {
                sql.push_str(&format!("\nON CONFLICT ({pk_list}) DO NOTHING"));
            } else {
                let update_set = non_pk_columns
                    .iter()
                    .map(|c| {
                        let qc = quote_identifier(c, db_type);
                        format!("{qc} = EXCLUDED.{qc}")
                    })
                    .collect::<Vec<_>>()
                    .join(", ");
                sql.push_str(&format!("\nON CONFLICT ({pk_list}) DO UPDATE SET {update_set}"));
            }
            sql
        }
        DatabaseType::Mysql | DatabaseType::Doris | DatabaseType::StarRocks => {
            let mut sql = format!("INSERT INTO {full_table} ({col_list}) VALUES\n{}", value_rows.join(",\n"));
            if non_pk_columns.is_empty() {
                sql.push_str("\nON DUPLICATE KEY UPDATE ");
                let first_pk = quote_identifier(&pk_columns[0], db_type);
                sql.push_str(&format!("{first_pk} = {first_pk}"));
            } else {
                let update_set = non_pk_columns
                    .iter()
                    .map(|c| {
                        let qc = quote_identifier(c, db_type);
                        format!("{qc} = VALUES({qc})")
                    })
                    .collect::<Vec<_>>()
                    .join(", ");
                sql.push_str(&format!("\nON DUPLICATE KEY UPDATE {update_set}"));
            }
            sql
        }
        DatabaseType::SqlServer => {
            let src_col_list = columns.iter().map(|c| quote_identifier(c, db_type)).collect::<Vec<_>>().join(", ");
            let on_clause = pk_columns
                .iter()
                .map(|c| {
                    let qc = quote_identifier(c, db_type);
                    format!("target.{qc} = src.{qc}")
                })
                .collect::<Vec<_>>()
                .join(" AND ");

            let mut sql = format!(
                "MERGE INTO {full_table} AS target USING (VALUES\n{}\n) AS src ({src_col_list}) ON {on_clause}",
                value_rows.join(",\n")
            );

            if !non_pk_columns.is_empty() {
                let update_set = non_pk_columns
                    .iter()
                    .map(|c| {
                        let qc = quote_identifier(c, db_type);
                        format!("target.{qc} = src.{qc}")
                    })
                    .collect::<Vec<_>>()
                    .join(", ");
                sql.push_str(&format!("\nWHEN MATCHED THEN UPDATE SET {update_set}"));
            }

            let insert_cols = columns.iter().map(|c| quote_identifier(c, db_type)).collect::<Vec<_>>().join(", ");
            let insert_vals =
                columns.iter().map(|c| format!("src.{}", quote_identifier(c, db_type))).collect::<Vec<_>>().join(", ");
            sql.push_str(&format!("\nWHEN NOT MATCHED THEN INSERT ({insert_cols}) VALUES ({insert_vals});"));
            sql
        }
        DatabaseType::Oracle => {
            let mut using_rows = Vec::with_capacity(rows.len());
            for row in rows {
                let mut vals = Vec::with_capacity(row.len().min(columns.len()));
                for (index, (v, c)) in row.iter().zip(columns.iter()).enumerate() {
                    vals.push(format!(
                        "{} AS {}",
                        escape_value_typed(v, db_type, column_types.get(index).and_then(|value| value.as_deref())),
                        quote_identifier(c, db_type)
                    ));
                }
                using_rows.push(format!("SELECT {} FROM dual", vals.join(", ")));
            }

            let on_clause = pk_columns
                .iter()
                .map(|c| {
                    let qc = quote_identifier(c, db_type);
                    format!("t.{qc} = s.{qc}")
                })
                .collect::<Vec<_>>()
                .join(" AND ");

            let mut sql =
                format!("MERGE INTO {full_table} t USING ({}) s ON ({on_clause})", using_rows.join(" UNION ALL "));

            if !non_pk_columns.is_empty() {
                let update_set = non_pk_columns
                    .iter()
                    .map(|c| {
                        let qc = quote_identifier(c, db_type);
                        format!("t.{qc} = s.{qc}")
                    })
                    .collect::<Vec<_>>()
                    .join(", ");
                sql.push_str(&format!("\nWHEN MATCHED THEN UPDATE SET {update_set}"));
            }

            let insert_cols = columns.iter().map(|c| quote_identifier(c, db_type)).collect::<Vec<_>>().join(", ");
            let insert_vals =
                columns.iter().map(|c| format!("s.{}", quote_identifier(c, db_type))).collect::<Vec<_>>().join(", ");
            sql.push_str(&format!("\nWHEN NOT MATCHED THEN INSERT ({insert_cols}) VALUES ({insert_vals})"));
            sql
        }
        _ => generate_insert_typed(columns, column_types, rows, table, schema, db_type),
    }
}

pub fn pagination_sql(
    columns: &[String],
    table: &str,
    schema: &str,
    db_type: &DatabaseType,
    offset: u64,
    limit: usize,
) -> String {
    let full_table = qualified_table(table, schema, db_type);
    let col_list = columns.iter().map(|c| quote_identifier(c, db_type)).collect::<Vec<_>>().join(", ");

    match db_type {
        DatabaseType::SqlServer | DatabaseType::Oracle => {
            format!(
                "SELECT {col_list} FROM {full_table} ORDER BY (SELECT NULL) OFFSET {offset} ROWS FETCH NEXT {limit} ROWS ONLY"
            )
        }
        _ => {
            format!("SELECT {col_list} FROM {full_table} LIMIT {limit} OFFSET {offset}")
        }
    }
}

pub fn count_sql(table: &str, schema: &str, db_type: &DatabaseType) -> String {
    let full_table = qualified_table(table, schema, db_type);
    format!("SELECT COUNT(*) FROM {full_table}")
}

pub async fn execute_on_pool(state: &AppState, pool_key: &str, sql: &str) -> Result<db::QueryResult, String> {
    let connections = state.connections.read().await;
    let pool = connections.get(pool_key).ok_or("Connection not found")?;

    match pool {
        PoolKind::Mysql(p, mode) => {
            let p = p.clone();
            let bare = *mode == crate::connection::MysqlMode::Bare;
            drop(connections);
            db::mysql::execute_query(&p, sql, bare).await
        }
        PoolKind::Postgres(p) => {
            let p = p.clone();
            drop(connections);
            db::postgres::execute_query(&p, sql).await
        }
        PoolKind::Sqlite(p) => {
            let p = p.clone();
            drop(connections);
            db::sqlite::execute_query(&p, sql).await
        }
        PoolKind::ClickHouse(client) => {
            let client = client.clone();
            let database = database_from_pool_key(pool_key).unwrap_or("default").to_string();
            drop(connections);
            db::clickhouse_driver::execute_query(&client, &database, sql).await
        }
        PoolKind::SqlServer(client) => {
            let client = client.clone();
            drop(connections);
            let mut client = client.lock().await;
            db::sqlserver::execute_query(&mut client, sql).await
        }
        PoolKind::Agent(client) => {
            let client = client.clone();
            let database = database_from_pool_key(pool_key).map(str::to_string);
            let sql = sql.to_string();
            drop(connections);
            let mut client = client.lock().await;
            let params = agent_execute_query_params(
                &sql,
                database.as_deref(),
                None,
                QueryExecutionOptions { max_rows: None, ..QueryExecutionOptions::default() },
            );
            client.execute_query(params).await
        }
        PoolKind::DuckDb(con) => {
            let con = con.clone();
            let sql = sql.to_string();
            drop(connections);
            tokio::task::spawn_blocking(move || {
                let con = con.lock().map_err(|e| e.to_string())?;
                let start = std::time::Instant::now();
                let trimmed = sql.trim().to_uppercase();
                if trimmed.starts_with("SELECT")
                    || trimmed.starts_with("SHOW")
                    || trimmed.starts_with("DESCRIBE")
                    || trimmed.starts_with("WITH")
                    || trimmed.starts_with("PRAGMA")
                {
                    let mut stmt = con.prepare(&sql).map_err(|e| e.to_string())?;
                    let mut rows = stmt.query([]).map_err(|e| e.to_string())?;
                    let stmt_ref = rows.as_ref().ok_or("DuckDB statement unavailable")?;
                    let col_count = stmt_ref.column_count();
                    let columns: Vec<String> = (0..col_count)
                        .map(|i| stmt_ref.column_name(i).map(|s| s.to_string()).unwrap_or_else(|_| "?".to_string()))
                        .collect();
                    let mut result_rows = Vec::new();
                    while let Some(row) = rows.next().map_err(|e| e.to_string())? {
                        let vals: Vec<serde_json::Value> = (0..col_count)
                            .map(|i| {
                                row.get::<_, String>(i)
                                    .map(serde_json::Value::String)
                                    .or_else(|_| row.get::<_, i64>(i).map(|v| serde_json::Value::Number(v.into())))
                                    .or_else(|_| {
                                        row.get::<_, f64>(i).map(|v| {
                                            serde_json::Number::from_f64(v)
                                                .map(serde_json::Value::Number)
                                                .unwrap_or(serde_json::Value::Null)
                                        })
                                    })
                                    .or_else(|_| row.get::<_, bool>(i).map(serde_json::Value::Bool))
                                    .unwrap_or(serde_json::Value::Null)
                            })
                            .collect();
                        result_rows.push(vals);
                    }
                    Ok(db::QueryResult {
                        columns,
                        rows: result_rows,
                        affected_rows: 0,
                        execution_time_ms: start.elapsed().as_millis(),
                        truncated: false,
                        session_id: None,
                        has_more: false,
                    })
                } else {
                    let affected = con.execute(&sql, []).map_err(|e| e.to_string())?;
                    Ok(db::QueryResult {
                        columns: vec![],
                        rows: vec![],
                        affected_rows: affected as u64,
                        execution_time_ms: start.elapsed().as_millis(),
                        truncated: false,
                        session_id: None,
                        has_more: false,
                    })
                }
            })
            .await
            .map_err(|e| e.to_string())?
        }
        PoolKind::ExternalTabular(ext_pool) => {
            let con = ext_pool.cache.clone();
            let sql = sql.to_string();
            drop(connections);
            tokio::task::spawn_blocking(move || {
                let con = con.lock().map_err(|e| e.to_string())?;
                crate::query::duckdb_execute(&con, &sql)
            })
            .await
            .map_err(|e| e.to_string())?
        }
        _ => Err("Unsupported database type for transfer".to_string()),
    }
}

fn database_from_pool_key(pool_key: &str) -> Option<&str> {
    pool_key
        .split_once(":session:")
        .map(|(base, _)| base)
        .unwrap_or(pool_key)
        .split_once(':')
        .map(|(_, database)| database)
        .filter(|database| !database.is_empty())
}

pub async fn get_db_type(state: &AppState, connection_id: &str) -> Result<DatabaseType, String> {
    let configs = state.configs.read().await;
    configs
        .get(connection_id)
        .map(|c| c.db_type.clone())
        .ok_or_else(|| format!("Connection config not found: {connection_id}"))
}

pub async fn get_columns_for_transfer(
    state: &AppState,
    pool_key: &str,
    _connection_id: &str,
    database: &str,
    schema: &str,
    table: &str,
) -> Result<Vec<db::ColumnInfo>, String> {
    let connections = state.connections.read().await;

    if let Some(PoolKind::DuckDb(con)) = connections.get(pool_key) {
        let con = con.clone();
        drop(connections);
        let table = table.to_string();
        let schema = schema.to_string();
        return tokio::task::spawn_blocking(move || {
            let con = con.lock().map_err(|e| e.to_string())?;
            crate::schema::duckdb_query_columns_in_database(&con, "main", &schema, &table)
        })
        .await
        .map_err(|e| e.to_string())?;
    }

    if let Some(PoolKind::ExternalTabular(ext_pool)) = connections.get(pool_key) {
        let con = ext_pool.cache.clone();
        drop(connections);
        let table = table.to_string();
        let schema = schema.to_string();
        return tokio::task::spawn_blocking(move || {
            let con = con.lock().map_err(|e| e.to_string())?;
            crate::schema::duckdb_query_columns_in_database(&con, "main", &schema, &table)
        })
        .await
        .map_err(|e| e.to_string())?;
    }

    if let Some(PoolKind::ClickHouse(client)) = connections.get(pool_key) {
        let client = client.clone();
        let database = database.to_string();
        let table = table.to_string();
        drop(connections);
        return db::clickhouse_driver::get_columns(&client, &database, &table).await;
    }
    if let Some(PoolKind::SqlServer(client)) = connections.get(pool_key) {
        let client = client.clone();
        let schema = schema.to_string();
        let table = table.to_string();
        drop(connections);
        let mut client = client.lock().await;
        return db::sqlserver::get_columns(&mut client, &schema, &table).await;
    }
    if let Some(PoolKind::Agent(client)) = connections.get(pool_key) {
        let client = client.clone();
        let database = database.to_string();
        let schema = schema.to_string();
        let table = table.to_string();
        drop(connections);
        let mut client = client.lock().await;
        return client.get_columns(&database, &schema, &table).await;
    }
    let pool = connections.get(pool_key).ok_or("Pool not found")?;
    let schema = schema.to_string();
    let table = table.to_string();
    match pool {
        PoolKind::Mysql(p, _) => {
            let p = p.clone();
            drop(connections);
            db::mysql::get_columns(&p, &schema, &table).await
        }
        PoolKind::Postgres(p) => {
            let p = p.clone();
            drop(connections);
            db::postgres::get_columns(&p, &schema, &table).await
        }
        PoolKind::Sqlite(p) => {
            let p = p.clone();
            drop(connections);
            db::sqlite::get_columns(&p, &schema, &table).await
        }
        _ => Err("Unsupported database type".to_string()),
    }
}

pub async fn is_cancelled(transfer_id: &str) -> bool {
    CANCELLED.read().await.contains(transfer_id)
}

pub async fn set_cancelled(transfer_id: &str) {
    CANCELLED.write().await.insert(transfer_id.to_string());
}

pub async fn clear_cancelled(transfer_id: &str) {
    CANCELLED.write().await.remove(transfer_id);
}

/// Transfer a single table. Returns rows transferred.
/// `progress_callback` is invoked for progress updates.
pub async fn transfer_table<F>(
    state: &AppState,
    request: &TransferRequest,
    table: &str,
    table_index: usize,
    source_db_type: &DatabaseType,
    target_db_type: &DatabaseType,
    source_pool_key: &str,
    target_pool_key: &str,
    mut progress_callback: F,
) -> Result<u64, String>
where
    F: FnMut(TransferProgress),
{
    let total_tables = request.tables.len();

    // Get source columns (deduplicate by name)
    let columns = {
        let raw = get_columns_for_transfer(
            state,
            source_pool_key,
            &request.source_connection_id,
            &request.source_database,
            &request.source_schema,
            table,
        )
        .await?;
        let mut seen = std::collections::HashSet::new();
        raw.into_iter().filter(|c| seen.insert(c.name.clone())).collect::<Vec<_>>()
    };

    if columns.is_empty() {
        return Err(format!("No columns found for table {table}"));
    }

    let col_names: Vec<String> = columns.iter().map(|c| c.name.clone()).collect();
    let col_types: Vec<Option<String>> = columns.iter().map(|c| Some(c.data_type.clone())).collect();
    log::info!("[transfer] {} has {} columns, counting rows...", table, columns.len());

    // Fetch source table comment
    let table_comment: Option<String> = crate::schema::list_tables_core(
        state,
        &request.source_connection_id,
        &request.source_database,
        &request.source_schema,
        Some(table),
        Some(1),
    )
    .await
    .unwrap_or_default()
    .into_iter()
    .next()
    .and_then(|t| t.comment);

    // Count source rows
    let total_rows = {
        let sql = count_sql(table, &request.source_schema, source_db_type);
        match execute_on_pool(state, source_pool_key, &sql).await {
            Ok(result) => result.rows.first().and_then(|r| r.first()).and_then(|v| match v {
                serde_json::Value::Number(n) => n.as_u64(),
                serde_json::Value::String(s) => s.parse::<u64>().ok(),
                _ => None,
            }),
            Err(e) => {
                log::warn!("[transfer] count failed for {}: {}", table, e);
                None
            }
        }
    };
    log::info!("[transfer] {} total_rows={:?}", table, total_rows);

    // Create table on target if requested
    if request.create_table {
        let ddl = generate_create_table_ddl(
            &columns,
            table,
            &request.target_schema,
            target_db_type,
            source_db_type,
            table_comment.as_deref(),
        );
        log::info!("[transfer] creating target table: {}", &ddl[..ddl.len().min(200)]);
        let table_exists = match execute_on_pool(state, target_pool_key, &ddl).await {
            Ok(_) => true,
            Err(e) => {
                let err_lower = e.to_lowercase();
                if err_lower.contains("already exists") || err_lower.contains("there is already") {
                    true
                } else {
                    return Err(format!("Failed to create table: {e}"));
                }
            }
        };
        if table_exists {
            let comment_stmts =
                generate_comment_ddl(&columns, table, &request.target_schema, target_db_type, table_comment.as_deref());
            for stmt in &comment_stmts {
                if let Err(e) = execute_on_pool(state, target_pool_key, stmt).await {
                    log::warn!("[transfer] failed to set column comment for {}: {}", table, e);
                }
            }
        }
    }

    // Truncate target if overwrite mode
    if request.mode == TransferMode::Overwrite {
        let full_table = qualified_table(table, &request.target_schema, target_db_type);
        let truncate_sql = match target_db_type {
            DatabaseType::Sqlite | DatabaseType::DuckDb => format!("DELETE FROM {full_table}"),
            _ => format!("TRUNCATE TABLE {full_table}"),
        };
        execute_on_pool(state, target_pool_key, &truncate_sql).await.map_err(|e| format!("Failed to truncate: {e}"))?;
    }

    // Determine effective mode and PK columns for upsert
    let (effective_mode, pk_columns) = if request.mode == TransferMode::Upsert {
        if matches!(target_db_type, DatabaseType::ClickHouse) {
            log::warn!("[transfer] upsert not supported for ClickHouse, falling back to append");
            (TransferMode::Append, vec![])
        } else {
            let target_columns = get_columns_for_transfer(
                state,
                target_pool_key,
                &request.target_connection_id,
                &request.target_database,
                &request.target_schema,
                table,
            )
            .await
            .unwrap_or_default();
            let pks: Vec<String> = target_columns.iter().filter(|c| c.is_primary_key).map(|c| c.name.clone()).collect();
            if pks.is_empty() {
                log::warn!("[transfer] table {} has no primary key, falling back to append", table);
                (TransferMode::Append, vec![])
            } else {
                (TransferMode::Upsert, pks)
            }
        }
    } else {
        (request.mode.clone(), vec![])
    };

    // Transfer data in batches
    let batch_size = if request.batch_size == 0 { 1000 } else { request.batch_size };
    let mut offset: u64 = 0;
    let mut total_transferred: u64 = 0;

    loop {
        if is_cancelled(&request.transfer_id).await {
            return Err("Cancelled".to_string());
        }

        let sql = pagination_sql(&col_names, table, &request.source_schema, source_db_type, offset, batch_size);
        let result = execute_on_pool(state, source_pool_key, &sql).await?;
        let row_count = result.rows.len();

        if row_count == 0 {
            break;
        }

        let batch_sql = match effective_mode {
            TransferMode::Upsert => generate_upsert_typed(
                &col_names,
                &col_types,
                &result.rows,
                table,
                &request.target_schema,
                target_db_type,
                &pk_columns,
            ),
            _ => generate_insert_typed(
                &col_names,
                &col_types,
                &result.rows,
                table,
                &request.target_schema,
                target_db_type,
            ),
        };
        if !batch_sql.is_empty() {
            execute_on_pool(state, target_pool_key, &batch_sql)
                .await
                .map_err(|e| format!("Insert failed at offset {offset}: {e}"))?;
        }

        total_transferred += row_count as u64;
        log::info!("[transfer] {} batch +{} rows (total {})", table, row_count, total_transferred);
        offset += row_count as u64;

        progress_callback(TransferProgress {
            transfer_id: request.transfer_id.clone(),
            table: table.to_string(),
            table_index,
            total_tables,
            rows_transferred: total_transferred,
            total_rows,
            status: TransferStatus::Running,
            error: None,
        });

        if row_count < batch_size {
            break;
        }
    }

    Ok(total_transferred)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::connection::{AppState, PoolKind};
    use crate::storage::Storage;
    use serde_json::json;
    use std::sync::Arc;

    fn duckdb_test_config(id: &str) -> crate::models::connection::ConnectionConfig {
        crate::models::connection::ConnectionConfig {
            id: id.to_string(),
            name: id.to_string(),
            db_type: DatabaseType::DuckDb,
            driver_profile: None,
            driver_label: None,
            url_params: None,
            host: ":memory:".to_string(),
            port: 0,
            username: String::new(),
            password: String::new(),
            database: None,
            visible_databases: None,
            attached_databases: Vec::new(),
            color: None,
            ssh_enabled: false,
            ssh_host: String::new(),
            ssh_port: 22,
            ssh_user: String::new(),
            ssh_password: String::new(),
            ssh_key_path: String::new(),
            ssh_key_passphrase: String::new(),
            ssh_expose_lan: false,
            ssh_connect_timeout_secs: 5,
            proxy_enabled: false,
            proxy_type: crate::models::connection::ProxyType::Socks5,
            proxy_host: String::new(),
            proxy_port: 1080,
            proxy_username: String::new(),
            proxy_password: String::new(),
            ssl: false,
            ca_cert_path: String::new(),
            sysdba: false,
            oracle_connection_type: None,
            connection_string: None,
            redis_connection_mode: None,
            redis_sentinel_master: String::new(),
            redis_sentinel_nodes: String::new(),
            redis_sentinel_username: String::new(),
            redis_sentinel_password: String::new(),
            redis_sentinel_tls: false,
            external_config: None,
            jdbc_driver_class: None,
            jdbc_driver_paths: Vec::new(),
            one_time: false,
        }
    }

    fn test_column(name: &str, data_type: &str) -> db::ColumnInfo {
        db::ColumnInfo {
            name: name.to_string(),
            data_type: data_type.to_string(),
            is_nullable: true,
            column_default: None,
            is_primary_key: false,
            extra: None,
            comment: None,
            numeric_precision: None,
            numeric_scale: None,
            character_maximum_length: None,
        }
    }

    #[test]
    fn mysql_create_table_includes_column_comments() {
        let cols = vec![
            db::ColumnInfo { comment: Some("用户ID".to_string()), is_primary_key: true, ..test_column("id", "int") },
            db::ColumnInfo {
                comment: Some("用户姓名".to_string()),
                is_nullable: false,
                ..test_column("name", "varchar(100)")
            },
            db::ColumnInfo { comment: None, ..test_column("age", "int") },
        ];

        let ddl = generate_create_table_ddl(&cols, "users", "", &DatabaseType::Mysql, &DatabaseType::Mysql, None);

        assert!(ddl.contains("COMMENT '用户ID'"));
        assert!(ddl.contains("COMMENT '用户姓名'"));
        assert!(!ddl.contains("`age` INT COMMENT")); // no comment for age
        assert!(ddl.contains("`name` VARCHAR(100) NOT NULL COMMENT '用户姓名'"));
        assert!(ddl.contains("PRIMARY KEY (`id`)"));
    }

    #[test]
    fn mysql_create_table_includes_table_comment() {
        let cols = vec![db::ColumnInfo { is_primary_key: true, ..test_column("id", "int") }];

        let ddl =
            generate_create_table_ddl(&cols, "users", "", &DatabaseType::Mysql, &DatabaseType::Mysql, Some("用户表"));

        assert!(ddl.contains(") COMMENT='用户表'"));
    }

    #[test]
    fn mysql_text_pk_gets_key_prefix() {
        let cols =
            vec![db::ColumnInfo { data_type: "text".to_string(), is_primary_key: true, ..test_column("id", "text") }];

        let ddl = generate_create_table_ddl(&cols, "logs", "", &DatabaseType::Mysql, &DatabaseType::Sqlite, None);

        assert!(ddl.contains("PRIMARY KEY (`id`(255))"));
        assert!(ddl.contains("`id` TEXT"));
    }

    #[test]
    fn mysql_int_pk_no_prefix() {
        let cols = vec![db::ColumnInfo { is_primary_key: true, ..test_column("id", "int") }];

        let ddl = generate_create_table_ddl(&cols, "users", "", &DatabaseType::Mysql, &DatabaseType::Sqlite, None);

        assert!(ddl.contains("PRIMARY KEY (`id`)"));
        assert!(!ddl.contains("PRIMARY KEY (`id`(255))"));
    }

    #[test]
    fn postgres_comment_ddl_generates_column_and_table_comments() {
        let cols = vec![
            db::ColumnInfo { comment: Some("主键".to_string()), ..test_column("id", "int") },
            db::ColumnInfo { comment: Some("名称".to_string()), ..test_column("name", "varchar(100)") },
        ];

        let stmts = generate_comment_ddl(&cols, "items", "public", &DatabaseType::Postgres, Some("项目表"));

        assert_eq!(stmts.len(), 3);
        assert!(stmts[0].contains("COMMENT ON TABLE \"public\".\"items\" IS '项目表'"));
        assert!(stmts[1].contains("COMMENT ON COLUMN \"public\".\"items\".\"id\" IS '主键'"));
        assert!(stmts[2].contains("COMMENT ON COLUMN \"public\".\"items\".\"name\" IS '名称'"));
    }

    #[test]
    fn clickhouse_comment_ddl_uses_alter_table() {
        let cols = vec![db::ColumnInfo { comment: Some("日志消息".to_string()), ..test_column("message", "text") }];

        let stmts = generate_comment_ddl(&cols, "logs", "", &DatabaseType::ClickHouse, None);

        assert_eq!(stmts.len(), 1);
        assert!(stmts[0].contains("ALTER TABLE `logs` COMMENT COLUMN `message` '日志消息'"));
    }

    #[test]
    fn pg_comment_ddl_skips_empty_comments() {
        let cols = vec![
            db::ColumnInfo { comment: None, ..test_column("id", "int") },
            db::ColumnInfo { comment: Some("  ".to_string()), ..test_column("name", "varchar(100)") },
        ];

        let stmts = generate_comment_ddl(&cols, "t", "", &DatabaseType::Postgres, None);

        assert!(stmts.is_empty());
    }

    #[test]
    fn non_mysql_family_no_inline_comment() {
        let cols = vec![db::ColumnInfo { comment: Some("test".to_string()), ..test_column("col", "text") }];

        // PostgreSQL target should NOT have inline COMMENT
        let ddl = generate_create_table_ddl(&cols, "t", "", &DatabaseType::Postgres, &DatabaseType::Postgres, None);
        assert!(!ddl.contains("COMMENT"));
    }

    #[test]
    fn mysql_insert_normalizes_rfc3339_datetime_strings() {
        let sql = generate_insert_typed(
            &[String::from("insurance_start_time")],
            &[Some(String::from("datetime"))],
            &[vec![json!("2026-05-12T00:00:00+00:00")]],
            "policies",
            "",
            &DatabaseType::Mysql,
        );

        assert_eq!(sql, "INSERT INTO `policies` (`insurance_start_time`) VALUES\n('2026-05-12 00:00:00')");
    }

    #[test]
    fn mysql_insert_uses_column_types_for_temporal_literals() {
        let sql = generate_insert_typed(
            &[String::from("dt"), String::from("raw_text"), String::from("d"), String::from("t")],
            &[
                Some(String::from("datetime")),
                Some(String::from("varchar(64)")),
                Some(String::from("date")),
                Some(String::from("time")),
            ],
            &[vec![
                json!("2026-05-12T00:00:00+00:00"),
                json!("2026-05-12T00:00:00+00:00"),
                json!("2026-05-12T00:00:00+00:00"),
                json!("2026-05-12T09:30:45+00:00"),
            ]],
            "policies",
            "",
            &DatabaseType::Mysql,
        );

        assert_eq!(
            sql,
            "INSERT INTO `policies` (`dt`, `raw_text`, `d`, `t`) VALUES\n('2026-05-12 00:00:00', '2026-05-12T00:00:00+00:00', '2026-05-12', '09:30:45')"
        );
    }

    #[tokio::test]
    async fn duckdb_transfer_columns_use_requested_schema() {
        let dir = std::env::temp_dir().join(format!("dbx-transfer-test-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let storage = Storage::open(&dir.join("storage.db")).await.unwrap();
        let con = duckdb::Connection::open_in_memory().unwrap();
        con.execute_batch("CREATE SCHEMA analytics; CREATE TABLE analytics.items(id INTEGER);").unwrap();

        let state = AppState::new(storage);
        let con = Arc::new(std::sync::Mutex::new(con));
        state.connections.write().await.insert("duckdb-1".to_string(), PoolKind::DuckDb(con));
        state.configs.write().await.insert("duckdb-1".to_string(), duckdb_test_config("duckdb-1"));

        let columns =
            get_columns_for_transfer(&state, "duckdb-1", "duckdb-1", "main", "analytics", "items").await.unwrap();

        assert_eq!(columns.iter().map(|c| c.name.as_str()).collect::<Vec<_>>(), vec!["id"]);
    }

    #[test]
    fn database_from_pool_key_handles_session_scoped_keys() {
        assert_eq!(database_from_pool_key("conn:analytics"), Some("analytics"));
        assert_eq!(database_from_pool_key("conn:analytics:session:editor-1"), Some("analytics"));
        assert_eq!(database_from_pool_key("conn"), None);
    }
}
