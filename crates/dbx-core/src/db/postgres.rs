use chrono::{DateTime, NaiveDate, NaiveDateTime, NaiveTime, Utc};
use futures::StreamExt;
use percent_encoding::percent_decode_str;
use rust_decimal::Decimal;
use sqlx::postgres::{PgPool, PgPoolOptions, PgRow};
use sqlx::{Column, Executor, Row, TypeInfo, ValueRef};
use std::time::{Duration, Instant};

use super::file_validator::validate_file_path;
use crate::sql::starts_with_executable_sql_keyword;
use crate::types::{
    ColumnInfo, DatabaseInfo, ForeignKeyInfo, IndexInfo, ObjectInfo, QueryResult, TableInfo, TriggerInfo,
};

fn pg_temporal_to_json_value(row: &PgRow, idx: usize) -> Option<serde_json::Value> {
    if let Ok(v) = row.try_get::<DateTime<Utc>, _>(idx) {
        return Some(serde_json::Value::String(v.to_rfc3339()));
    }
    if let Ok(v) = row.try_get::<NaiveDateTime, _>(idx) {
        return Some(serde_json::Value::String(v.to_string()));
    }
    if let Ok(v) = row.try_get::<NaiveDate, _>(idx) {
        return Some(serde_json::Value::String(v.to_string()));
    }
    if let Ok(v) = row.try_get::<NaiveTime, _>(idx) {
        return Some(serde_json::Value::String(v.to_string()));
    }
    None
}

fn pg_value_to_json(row: &PgRow, idx: usize, type_name: &str) -> serde_json::Value {
    if row.try_get_raw(idx).map(|v| v.is_null()).unwrap_or(true) {
        return serde_json::Value::Null;
    }

    let upper = type_name.to_uppercase();

    if upper == "JSON" || upper == "JSONB" {
        if let Ok(v) = row.try_get::<serde_json::Value, _>(idx) {
            return serde_json::Value::String(v.to_string());
        }
        if let Ok(v) = row.try_get::<String, _>(idx) {
            return serde_json::Value::String(v);
        }
        return serde_json::Value::Null;
    }

    if upper == "BOOL" {
        return row.try_get::<bool, _>(idx).map(serde_json::Value::Bool).unwrap_or(serde_json::Value::Null);
    }

    if upper.contains("TIMESTAMP")
        || upper == "DATE"
        || upper == "TIME"
        || upper == "TIMETZ"
        || upper.contains("INTERVAL")
    {
        if let Some(v) = pg_temporal_to_json_value(row, idx) {
            return v;
        }
    }

    if upper == "NUMERIC" || upper == "DECIMAL" || upper == "MONEY" {
        return row
            .try_get::<Decimal, _>(idx)
            .map(|v: Decimal| serde_json::Value::String(v.to_string()))
            .unwrap_or(serde_json::Value::Null);
    }

    if upper == "UUID" {
        return row
            .try_get::<uuid::Uuid, _>(idx)
            .map(|v| serde_json::Value::String(v.to_string()))
            .unwrap_or(serde_json::Value::Null);
    }

    row.try_get::<String, _>(idx)
        .map(serde_json::Value::String)
        .or_else(|_| row.try_get::<i64, _>(idx).map(super::safe_i64_to_json))
        .or_else(|_| row.try_get::<i32, _>(idx).map(|v| serde_json::Value::Number(v.into())))
        .or_else(|_| row.try_get::<i16, _>(idx).map(|v| serde_json::Value::Number(v.into())))
        .or_else(|_| row.try_get::<i8, _>(idx).map(|v| serde_json::Value::Number(v.into())))
        .or_else(|_| {
            row.try_get::<Vec<i8>, _>(idx)
                .map(|v| serde_json::Value::Array(v.into_iter().map(|v| serde_json::Value::Number(v.into())).collect()))
        })
        .or_else(|_| {
            row.try_get::<Vec<i16>, _>(idx)
                .map(|v| serde_json::Value::Array(v.into_iter().map(|v| serde_json::Value::Number(v.into())).collect()))
        })
        .or_else(|_| {
            row.try_get::<Vec<i32>, _>(idx)
                .map(|v| serde_json::Value::Array(v.into_iter().map(|v| serde_json::Value::Number(v.into())).collect()))
        })
        .or_else(|_| {
            row.try_get::<Vec<i64>, _>(idx)
                .map(|v| serde_json::Value::Array(v.into_iter().map(|v| serde_json::Value::Number(v.into())).collect()))
        })
        .or_else(|_| {
            row.try_get::<f64, _>(idx).map(|v| {
                serde_json::Number::from_f64(v).map(serde_json::Value::Number).unwrap_or(serde_json::Value::Null)
            })
        })
        .or_else(|_| {
            row.try_get::<f32, _>(idx).map(|v| {
                serde_json::Number::from_f64((v as f64 * 1_000_000.0).round() / 1_000_000.0)
                    .map(serde_json::Value::Number)
                    .unwrap_or(serde_json::Value::Null)
            })
        })
        .or_else(|_| row.try_get::<bool, _>(idx).map(serde_json::Value::Bool))
        .or_else(|_| row.try_get::<uuid::Uuid, _>(idx).map(|v| serde_json::Value::String(v.to_string())))
        .or_else(|e| pg_temporal_to_json_value(row, idx).ok_or(e))
        .or_else(|_| {
            row.try_get_raw(idx).map(|raw| {
                if raw.is_null() {
                    return serde_json::Value::Null;
                }
                match raw.as_bytes() {
                    Ok(bytes) => match std::str::from_utf8(bytes) {
                        Ok(s) => serde_json::Value::String(s.to_string()),
                        Err(_) => {
                            let hex: String = bytes.iter().map(|b| format!("{:02x}", b)).collect();
                            serde_json::Value::String(hex)
                        }
                    },
                    Err(_) => serde_json::Value::Null,
                }
            })
        })
        .unwrap_or(serde_json::Value::Null)
}

pub async fn connect(url: &str) -> Result<PgPool, String> {
    // Validate SSL certificate paths if present in the URL
    validate_postgres_ssl_paths(url)?;

    let tz = iana_time_zone::get_timezone().unwrap_or_else(|_| "UTC".to_string());
    let url_owned = url.to_string();
    super::with_connection_timeout("PostgreSQL", async {
        PgPoolOptions::new()
            .max_connections(5)
            .acquire_timeout(super::connection_timeout())
            .idle_timeout(Duration::from_secs(300))
            .after_connect(move |conn, _meta| {
                let tz = tz.clone();
                Box::pin(async move {
                    conn.execute(sqlx::query(&format!("SET timezone = '{tz}'"))).await?;
                    Ok(())
                })
            })
            .connect(&url_owned)
            .await
            .map_err(|e| format!("PostgreSQL connection failed: {e}"))
    })
    .await
}

/// Validates SSL certificate file paths in PostgreSQL connection URLs.
///
/// PostgreSQL connection strings can include SSL parameters like:
/// - sslcert=/path/to/cert.pem
/// - sslkey=/path/to/key.pem
/// - sslrootcert=/path/to/root.pem
fn validate_postgres_ssl_paths(url: &str) -> Result<(), String> {
    // Extract query parameters from URL
    if let Some(query_start) = url.find('?') {
        let query_string = &url[query_start + 1..];

        for param in query_string.split('&') {
            if let Some((key, value)) = param.split_once('=') {
                match key {
                    "sslcert" | "sslkey" | "sslrootcert" => {
                        // URL decode the value
                        let decoded = percent_decode_str(value)
                            .decode_utf8()
                            .map_err(|_| format!("Invalid URL encoding in {key}"))?;

                        // Validate the file path (skip network paths)
                        validate_file_path(&decoded, |_| false)?;
                    }
                    _ => {}
                }
            }
        }
    }

    Ok(())
}

pub async fn list_databases(pool: &PgPool) -> Result<Vec<DatabaseInfo>, String> {
    let rows: Vec<PgRow> = sqlx::query(
        "SELECT datname FROM pg_database \
         WHERE datistemplate = false AND datallowconn = true \
         ORDER BY datname",
    )
    .fetch_all(pool)
    .await
    .map_err(|e| e.to_string())?;

    Ok(rows.iter().map(|row| DatabaseInfo { name: row.get::<String, _>("datname") }).collect())
}

pub async fn list_tables(pool: &PgPool, schema: &str) -> Result<Vec<TableInfo>, String> {
    let rows: Vec<PgRow> =
        sqlx::query(postgres_tables_sql()).bind(schema).fetch_all(pool).await.map_err(|e| e.to_string())?;

    Ok(rows
        .iter()
        .map(|row| TableInfo {
            name: row.get::<String, _>("table_name"),
            table_type: row.get::<String, _>("table_type"),
            comment: row.get::<Option<String>, _>("table_comment").filter(|s| !s.is_empty()),
        })
        .collect())
}

fn postgres_tables_sql() -> &'static str {
    "SELECT c.relname AS table_name, \
         CASE c.relkind WHEN 'r' THEN 'BASE TABLE' WHEN 'v' THEN 'VIEW' \
           WHEN 'm' THEN 'MATERIALIZED VIEW' WHEN 'f' THEN 'FOREIGN TABLE' \
           WHEN 'p' THEN 'BASE TABLE' END AS table_type, \
         obj_description(c.oid) AS table_comment \
         FROM pg_catalog.pg_class c \
         JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace \
         WHERE n.nspname = $1 AND c.relkind IN ('r','v','m','f','p') \
         ORDER BY c.relname"
}

fn get_opt_text(row: &PgRow, name: &str) -> Option<String> {
    row.try_get::<Option<String>, _>(name).ok().flatten().filter(|s| !s.is_empty())
}

fn list_objects_sql(include_timestamps: bool) -> &'static str {
    if include_timestamps {
        return "SELECT c.relname AS object_name, \
       CASE c.relkind \
         WHEN 'v' THEN 'VIEW' \
         WHEN 'm' THEN 'VIEW' \
         ELSE 'TABLE' \
       END AS object_type, \
       obj_description(c.oid) AS object_comment, \
       stat.creation::text AS created_at, \
       COALESCE( \
         CASE WHEN current_setting('track_commit_timestamp', true) = 'on' \
           THEN pg_xact_commit_timestamp(c.xmin)::text END, \
         stat.modification::text \
       ) AS updated_at, \
       CASE c.relkind WHEN 'v' THEN 1 WHEN 'm' THEN 1 ELSE 0 END AS sort_order \
     FROM pg_catalog.pg_class c \
     JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace \
     LEFT JOIN LATERAL pg_stat_file( \
       CASE WHEN c.relkind IN ('r','m','f','p') THEN pg_relation_filepath(c.oid) END, true \
     ) stat ON true \
     WHERE n.nspname = $1 AND c.relkind IN ('r','v','m','f','p') \
     UNION ALL \
     SELECT p.proname AS object_name, \
       CASE p.prokind WHEN 'p' THEN 'PROCEDURE' ELSE 'FUNCTION' END AS object_type, \
       obj_description(p.oid) AS object_comment, \
       NULL::text AS created_at, \
       CASE WHEN current_setting('track_commit_timestamp', true) = 'on' \
         THEN pg_xact_commit_timestamp(p.xmin)::text END AS updated_at, \
       CASE p.prokind WHEN 'p' THEN 2 ELSE 3 END AS sort_order \
     FROM pg_catalog.pg_proc p \
     JOIN pg_catalog.pg_namespace n ON n.oid = p.pronamespace \
     WHERE n.nspname = $1 AND p.prokind IN ('p','f') \
     ORDER BY sort_order, object_name";
    }

    "SELECT c.relname AS object_name, \
       CASE c.relkind \
         WHEN 'v' THEN 'VIEW' \
         WHEN 'm' THEN 'VIEW' \
         ELSE 'TABLE' \
       END AS object_type, \
       obj_description(c.oid) AS object_comment, \
       NULL::text AS created_at, \
       NULL::text AS updated_at, \
       CASE c.relkind WHEN 'v' THEN 1 WHEN 'm' THEN 1 ELSE 0 END AS sort_order \
     FROM pg_catalog.pg_class c \
     JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace \
     WHERE n.nspname = $1 AND c.relkind IN ('r','v','m','f','p') \
     UNION ALL \
     SELECT p.proname AS object_name, \
       CASE p.prokind WHEN 'p' THEN 'PROCEDURE' ELSE 'FUNCTION' END AS object_type, \
       obj_description(p.oid) AS object_comment, \
       NULL::text AS created_at, \
       NULL::text AS updated_at, \
       CASE p.prokind WHEN 'p' THEN 2 ELSE 3 END AS sort_order \
     FROM pg_catalog.pg_proc p \
     JOIN pg_catalog.pg_namespace n ON n.oid = p.pronamespace \
     WHERE n.nspname = $1 AND p.prokind IN ('p','f') \
     ORDER BY sort_order, object_name"
}

pub async fn list_objects(pool: &PgPool, schema: &str) -> Result<Vec<ObjectInfo>, String> {
    let rows: Vec<PgRow> = match sqlx::query(list_objects_sql(true)).bind(schema).fetch_all(pool).await {
        Ok(rows) => rows,
        Err(_) => sqlx::query(list_objects_sql(false)).bind(schema).fetch_all(pool).await.map_err(|e| e.to_string())?,
    };

    Ok(rows
        .iter()
        .map(|row| ObjectInfo {
            name: row.get::<String, _>("object_name"),
            object_type: row.get::<String, _>("object_type"),
            schema: Some(schema.to_string()),
            comment: row.get::<Option<String>, _>("object_comment").filter(|s| !s.is_empty()),
            created_at: get_opt_text(row, "created_at"),
            updated_at: get_opt_text(row, "updated_at"),
        })
        .collect())
}

pub async fn list_schemas(pool: &PgPool) -> Result<Vec<String>, String> {
    let rows: Vec<PgRow> = sqlx::query(
        "SELECT n.nspname AS schema_name FROM pg_catalog.pg_namespace n \
         WHERE n.nspname NOT IN ('information_schema', 'pg_catalog', 'pg_toast') \
         AND n.nspname NOT LIKE 'pg_toast_temp_%' \
         AND n.nspname NOT LIKE 'pg_temp_%' \
         ORDER BY n.nspname",
    )
    .fetch_all(pool)
    .await
    .map_err(|e| e.to_string())?;

    Ok(rows.iter().map(|row| row.get::<String, _>("schema_name")).collect())
}

pub async fn get_columns(pool: &PgPool, schema: &str, table: &str) -> Result<Vec<ColumnInfo>, String> {
    let rows: Vec<PgRow> = sqlx::query(
        "SELECT a.attname AS column_name, \
         format_type(a.atttypid, a.atttypmod) AS full_type, \
         NOT a.attnotnull AS is_nullable, \
         pg_get_expr(ad.adbin, ad.adrelid) AS column_default, \
         EXISTS ( \
           SELECT 1 FROM pg_constraint co \
           JOIN pg_index i ON i.indrelid = co.conrelid AND co.conindid = i.indexrelid \
           WHERE co.conrelid = a.attrelid AND co.contype = 'p' \
           AND a.attnum = ANY(i.indkey) \
         ) AS is_pk, \
         col_description(a.attrelid, a.attnum) AS column_comment, \
         CASE WHEN t.typname = 'numeric' AND a.atttypmod > 0 \
           THEN ((a.atttypmod - 4) >> 16) & 65535 ELSE NULL END AS numeric_precision, \
         CASE WHEN t.typname = 'numeric' AND a.atttypmod > 0 \
           THEN (a.atttypmod - 4) & 65535 ELSE NULL END AS numeric_scale, \
         CASE WHEN t.typname IN ('varchar', 'bpchar') AND a.atttypmod > 0 \
           THEN a.atttypmod - 4 ELSE NULL END AS character_maximum_length \
         FROM pg_attribute a \
         JOIN pg_type t ON t.oid = a.atttypid \
         LEFT JOIN pg_attrdef ad ON ad.adrelid = a.attrelid AND ad.adnum = a.attnum \
         WHERE a.attrelid = (quote_ident($1) || '.' || quote_ident($2))::regclass \
         AND a.attnum > 0 AND NOT a.attisdropped \
         ORDER BY a.attnum",
    )
    .bind(schema)
    .bind(table)
    .fetch_all(pool)
    .await
    .map_err(|e| e.to_string())?;

    Ok(rows
        .iter()
        .map(|row| {
            let full_type = row.get::<Option<String>, _>("full_type").unwrap_or_default();
            ColumnInfo {
                name: row.get::<String, _>("column_name"),
                data_type: full_type,
                is_nullable: row.get::<bool, _>("is_nullable"),
                column_default: row.get::<Option<String>, _>("column_default"),
                is_primary_key: row.get::<bool, _>("is_pk"),
                extra: None,
                comment: row.get::<Option<String>, _>("column_comment"),
                numeric_precision: row.get::<Option<i32>, _>("numeric_precision"),
                numeric_scale: row.get::<Option<i32>, _>("numeric_scale"),
                character_maximum_length: row.get::<Option<i32>, _>("character_maximum_length"),
            }
        })
        .collect())
}

fn query_result_row_limit(max_rows: Option<usize>) -> usize {
    max_rows.unwrap_or(crate::query::MAX_ROWS).max(1)
}

pub async fn execute_query(pool: &PgPool, sql: &str) -> Result<QueryResult, String> {
    execute_query_with_max_rows(pool, sql, None).await
}

pub async fn execute_query_with_max_rows(
    pool: &PgPool,
    sql: &str,
    max_rows: Option<usize>,
) -> Result<QueryResult, String> {
    let start = Instant::now();
    let row_limit = query_result_row_limit(max_rows);

    if starts_with_executable_sql_keyword(sql, &["SELECT", "SHOW", "EXPLAIN", "WITH", "TABLE"]) {
        let mut stream = sqlx::query(sql).persistent(false).fetch(pool);
        let mut columns: Vec<String> = vec![];
        let mut column_types: Vec<String> = vec![];
        let mut result_rows: Vec<Vec<serde_json::Value>> = Vec::new();

        while let Some(row) = stream.next().await {
            let row = row.map_err(|e| e.to_string())?;
            if columns.is_empty() {
                let cols = row.columns();
                columns = cols.iter().map(|c| c.name().to_string()).collect();
                column_types = cols.iter().map(|c| c.type_info().name().to_string()).collect();
            }
            result_rows.push(
                (0..row.len())
                    .map(|i| pg_value_to_json(&row, i, column_types.get(i).map(String::as_str).unwrap_or("")))
                    .collect(),
            );
            if result_rows.len() > row_limit {
                break;
            }
        }

        if columns.is_empty() {
            let desc = pool.describe(sql).await.map_err(|e| e.to_string())?;
            columns = desc.columns().iter().map(|c| c.name().to_string()).collect();
        }

        let truncated = result_rows.len() > row_limit;
        if truncated {
            result_rows.truncate(row_limit);
        }

        Ok(QueryResult {
            columns,
            rows: result_rows,
            affected_rows: 0,
            execution_time_ms: start.elapsed().as_millis(),
            truncated,
            session_id: None,
            has_more: false,
        })
    } else {
        let result = sqlx::query(sql).execute(pool).await.map_err(|e| e.to_string())?;

        Ok(QueryResult {
            columns: vec![],
            rows: vec![],
            affected_rows: result.rows_affected(),
            execution_time_ms: start.elapsed().as_millis(),
            truncated: false,
            session_id: None,
            has_more: false,
        })
    }
}

pub async fn execute_query_with_schema(pool: &PgPool, schema: &str, sql: &str) -> Result<QueryResult, String> {
    execute_query_with_schema_and_max_rows(pool, schema, sql, None).await
}

pub async fn execute_query_with_schema_and_max_rows(
    pool: &PgPool,
    schema: &str,
    sql: &str,
    max_rows: Option<usize>,
) -> Result<QueryResult, String> {
    let mut conn = pool.acquire().await.map_err(|e| e.to_string())?;
    let set_path = format!("SET search_path TO \"{}\", public", schema);
    sqlx::query(&set_path).execute(&mut *conn).await.map_err(|e| e.to_string())?;

    let start = Instant::now();
    let row_limit = query_result_row_limit(max_rows);

    if starts_with_executable_sql_keyword(sql, &["SELECT", "SHOW", "EXPLAIN", "WITH", "TABLE"]) {
        let mut stream = sqlx::query(sql).persistent(false).fetch(&mut *conn);
        let mut columns: Vec<String> = vec![];
        let mut column_types: Vec<String> = vec![];
        let mut result_rows: Vec<Vec<serde_json::Value>> = Vec::new();

        while let Some(row) = stream.next().await {
            let row = row.map_err(|e| e.to_string())?;
            if columns.is_empty() {
                let cols = row.columns();
                columns = cols.iter().map(|c| c.name().to_string()).collect();
                column_types = cols.iter().map(|c| c.type_info().name().to_string()).collect();
            }
            result_rows.push(
                (0..row.len())
                    .map(|i| pg_value_to_json(&row, i, column_types.get(i).map(String::as_str).unwrap_or("")))
                    .collect(),
            );
            if result_rows.len() > row_limit {
                break;
            }
        }
        drop(stream);

        if columns.is_empty() {
            let desc = (&mut *conn).describe(sql).await.map_err(|e| e.to_string())?;
            columns = desc.columns().iter().map(|c| c.name().to_string()).collect();
        }

        let truncated = result_rows.len() > row_limit;
        if truncated {
            result_rows.truncate(row_limit);
        }

        Ok(QueryResult {
            columns,
            rows: result_rows,
            affected_rows: 0,
            execution_time_ms: start.elapsed().as_millis(),
            truncated,
            session_id: None,
            has_more: false,
        })
    } else {
        let result = sqlx::query(sql).execute(&mut *conn).await.map_err(|e| e.to_string())?;

        Ok(QueryResult {
            columns: vec![],
            rows: vec![],
            affected_rows: result.rows_affected(),
            execution_time_ms: start.elapsed().as_millis(),
            truncated: false,
            session_id: None,
            has_more: false,
        })
    }
}

pub async fn list_indexes(pool: &PgPool, schema: &str, table: &str) -> Result<Vec<IndexInfo>, String> {
    let rows: Vec<PgRow> = sqlx::query(
        "SELECT i.relname AS index_name, \
         array_agg(COALESCE(a.attname, pg_get_indexdef(ix.indexrelid, k.n::int, true)) ORDER BY k.n) AS columns, \
         ix.indisunique AS is_unique, \
         ix.indisprimary AS is_primary, \
         pg_get_expr(ix.indpred, ix.indrelid) AS filter_expr, \
         am.amname AS index_type, \
         ix.indnkeyatts AS nkeyatts, \
         ix.indkey AS indkey, \
         obj_description(i.oid, 'pg_class') AS index_comment \
         FROM pg_index ix \
         JOIN pg_class t ON t.oid = ix.indrelid \
         JOIN pg_class i ON i.oid = ix.indexrelid \
         JOIN pg_namespace n ON n.oid = t.relnamespace \
         JOIN pg_am am ON am.oid = i.relam \
         JOIN LATERAL unnest(ix.indkey) WITH ORDINALITY AS k(attnum, n) ON true \
         LEFT JOIN pg_attribute a ON a.attrelid = t.oid AND a.attnum = k.attnum AND k.attnum > 0 \
         WHERE n.nspname = $1 AND t.relname = $2 \
         GROUP BY i.relname, i.oid, ix.indisunique, ix.indisprimary, ix.indpred, ix.indrelid, am.amname, ix.indnkeyatts, ix.indkey \
         ORDER BY i.relname",
    )
    .bind(schema)
    .bind(table)
    .fetch_all(pool)
    .await
    .map_err(|e| e.to_string())?;

    Ok(rows
        .iter()
        .map(|row| {
            let all_cols: Vec<String> = row.get::<Vec<String>, _>("columns");
            let nkeyatts = row.get::<Option<i16>, _>("nkeyatts").unwrap_or(all_cols.len() as i16) as usize;
            let key_cols = all_cols[..nkeyatts].to_vec();
            let included = if nkeyatts < all_cols.len() { all_cols[nkeyatts..].to_vec() } else { vec![] };
            IndexInfo {
                name: row.get::<String, _>("index_name"),
                columns: key_cols,
                is_unique: row.get::<bool, _>("is_unique"),
                is_primary: row.get::<bool, _>("is_primary"),
                filter: row.get::<Option<String>, _>("filter_expr"),
                index_type: row.get::<Option<String>, _>("index_type"),
                included_columns: if included.is_empty() { None } else { Some(included) },
                comment: row.get::<Option<String>, _>("index_comment"),
            }
        })
        .collect())
}

pub async fn list_foreign_keys(pool: &PgPool, schema: &str, table: &str) -> Result<Vec<ForeignKeyInfo>, String> {
    let rows: Vec<PgRow> = sqlx::query(
        "SELECT kcu.constraint_name, kcu.column_name, \
         ccu.table_name AS ref_table, ccu.column_name AS ref_column \
         FROM information_schema.key_column_usage kcu \
         JOIN information_schema.referential_constraints rc \
           ON kcu.constraint_name = rc.constraint_name \
           AND kcu.constraint_schema = rc.constraint_schema \
         JOIN information_schema.constraint_column_usage ccu \
           ON rc.unique_constraint_name = ccu.constraint_name \
           AND rc.unique_constraint_schema = ccu.constraint_schema \
         WHERE kcu.table_schema = $1 AND kcu.table_name = $2 \
         ORDER BY kcu.constraint_name",
    )
    .bind(schema)
    .bind(table)
    .fetch_all(pool)
    .await
    .map_err(|e| e.to_string())?;

    Ok(rows
        .iter()
        .map(|row| ForeignKeyInfo {
            name: row.get::<String, _>("constraint_name"),
            column: row.get::<String, _>("column_name"),
            ref_table: row.get::<String, _>("ref_table"),
            ref_column: row.get::<String, _>("ref_column"),
        })
        .collect())
}

pub async fn list_triggers(pool: &PgPool, schema: &str, table: &str) -> Result<Vec<TriggerInfo>, String> {
    let rows: Vec<PgRow> = sqlx::query(
        "SELECT trigger_name, event_manipulation, action_timing \
         FROM information_schema.triggers \
         WHERE trigger_schema = $1 AND event_object_table = $2 \
         ORDER BY trigger_name",
    )
    .bind(schema)
    .bind(table)
    .fetch_all(pool)
    .await
    .map_err(|e| e.to_string())?;

    Ok(rows
        .iter()
        .map(|row| TriggerInfo {
            name: row.get::<String, _>("trigger_name"),
            event: row.get::<String, _>("event_manipulation"),
            timing: row.get::<String, _>("action_timing"),
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn postgres_list_objects_sql_includes_routines() {
        let sql = list_objects_sql(true);

        assert!(sql.contains("pg_catalog.pg_class"));
        assert!(sql.contains("pg_catalog.pg_proc"));
        assert!(sql.contains("pg_stat_file"));
        assert!(sql.contains("pg_xact_commit_timestamp"));
        assert!(sql.contains("'PROCEDURE'"));
        assert!(sql.contains("'FUNCTION'"));
    }
}
