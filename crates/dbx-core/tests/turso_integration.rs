use dbx_core::db::turso_driver::TursoClient;
use std::time::Duration;

const TURSO_URL: &str = "http://172.20.66.143:20007";
const TURSO_TOKEN: &str = "";

fn try_client() -> Option<TursoClient> {
    TursoClient::new(TURSO_URL, TURSO_TOKEN, false, Duration::from_secs(5)).ok()
}

#[tokio::test]
async fn test_connection_works() {
    let Some(c) = try_client() else { return };
    dbx_core::db::turso_driver::test_connection(&c, Duration::from_secs(10)).await.expect("connection should work");
}

#[tokio::test]
async fn list_databases_returns_main() {
    let Some(c) = try_client() else { return };
    let dbs = dbx_core::db::turso_driver::list_databases(&c).await.expect("list databases");
    assert_eq!(dbs.len(), 1);
    assert_eq!(dbs[0].name, "main");
}

#[tokio::test]
async fn execute_query_select_one() {
    let Some(c) = try_client() else { return };
    let result = dbx_core::db::turso_driver::execute_query(&c, "SELECT 1 AS num").await.expect("query should work");
    assert_eq!(result.columns, vec!["num"]);
    assert_eq!(result.rows.len(), 1);
}

#[tokio::test]
async fn execute_query_with_table() {
    let Some(c) = try_client() else { return };
    dbx_core::db::turso_driver::execute_query(
        &c,
        "CREATE TABLE IF NOT EXISTS test_people (id INTEGER PRIMARY KEY, name TEXT)",
    )
    .await
    .expect("create table");

    dbx_core::db::turso_driver::execute_query(&c, "INSERT INTO test_people VALUES (1, 'Alice')").await.expect("insert");

    let result =
        dbx_core::db::turso_driver::execute_query(&c, "SELECT * FROM test_people ORDER BY id").await.expect("select");

    assert_eq!(result.columns, vec!["id", "name"]);
    assert_eq!(result.rows.len(), 1);

    dbx_core::db::turso_driver::execute_query(&c, "DROP TABLE test_people").await.expect("drop");
}

#[tokio::test]
async fn list_tables_works() {
    let Some(c) = try_client() else { return };
    dbx_core::db::turso_driver::execute_query(&c, "CREATE TABLE IF NOT EXISTS test_meta (x INTEGER)")
        .await
        .expect("create");

    let tables = dbx_core::db::turso_driver::list_tables(&c, "").await.expect("list tables");

    let has_test = tables.iter().any(|t| t.name == "test_meta");
    assert!(has_test, "should find test_meta table");

    dbx_core::db::turso_driver::execute_query(&c, "DROP TABLE test_meta").await.expect("drop");
}

#[tokio::test]
async fn get_columns_works() {
    let Some(c) = try_client() else { return };
    dbx_core::db::turso_driver::execute_query(
        &c,
        "CREATE TABLE IF NOT EXISTS test_cols (id INTEGER PRIMARY KEY, name TEXT NOT NULL, age INTEGER)",
    )
    .await
    .expect("create");

    let cols = dbx_core::db::turso_driver::get_columns(&c, "", "test_cols").await.expect("get columns");

    assert_eq!(cols.len(), 3);
    assert_eq!(cols[0].name, "id");
    assert_eq!(cols[1].name, "name");
    assert_eq!(cols[2].name, "age");
    assert!(cols[0].is_primary_key);
    assert!(!cols[1].is_nullable);

    dbx_core::db::turso_driver::execute_query(&c, "DROP TABLE test_cols").await.expect("drop");
}

#[tokio::test]
async fn list_indexes_works() {
    let Some(c) = try_client() else { return };
    dbx_core::db::turso_driver::execute_query(
        &c,
        "CREATE TABLE IF NOT EXISTS test_idx (id INTEGER PRIMARY KEY, email TEXT)",
    )
    .await
    .expect("create");
    dbx_core::db::turso_driver::execute_query(&c, "CREATE UNIQUE INDEX IF NOT EXISTS idx_email ON test_idx(email)")
        .await
        .expect("create index");

    let indexes = dbx_core::db::turso_driver::list_indexes(&c, "", "test_idx").await.expect("list indexes");

    let email_idx = indexes.iter().find(|i| i.name == "idx_email");
    assert!(email_idx.is_some(), "should find email index: {:?}", indexes);
    assert!(email_idx.unwrap().is_unique);

    dbx_core::db::turso_driver::execute_query(&c, "DROP TABLE test_idx").await.expect("drop");
}

#[tokio::test]
async fn table_ddl_works() {
    let Some(c) = try_client() else { return };
    dbx_core::db::turso_driver::execute_query(&c, "CREATE TABLE IF NOT EXISTS test_ddl (id INTEGER PRIMARY KEY)")
        .await
        .expect("create");

    let ddl = dbx_core::db::turso_driver::table_ddl(&c, "test_ddl").await.expect("get ddl");

    assert!(ddl.to_uppercase().contains("CREATE TABLE"), "DDL should contain CREATE TABLE: {ddl}");

    dbx_core::db::turso_driver::execute_query(&c, "DROP TABLE test_ddl").await.expect("drop");
}

#[tokio::test]
async fn multi_statement_transaction_works() {
    let Some(c) = try_client() else { return };
    dbx_core::db::turso_driver::execute_query(
        &c,
        "CREATE TABLE IF NOT EXISTS test_batch (id INTEGER PRIMARY KEY, val TEXT)",
    )
    .await
    .expect("create table");

    dbx_core::db::turso_driver::execute_query(
        &c,
        "BEGIN; INSERT INTO test_batch VALUES (1, 'a'); INSERT INTO test_batch VALUES (2, 'b'); COMMIT",
    )
    .await
    .expect("batch transaction");

    let result =
        dbx_core::db::turso_driver::execute_query(&c, "SELECT * FROM test_batch ORDER BY id").await.expect("select");
    assert_eq!(result.rows.len(), 2, "should have 2 rows after commit");

    dbx_core::db::turso_driver::execute_query(&c, "DROP TABLE test_batch").await.expect("drop");
}

#[tokio::test]
async fn standalone_commit_is_noop() {
    let Some(c) = try_client() else { return };
    let result =
        dbx_core::db::turso_driver::execute_query(&c, "COMMIT").await.expect("standalone COMMIT should not fail");
    assert_eq!(result.affected_rows, 0);
    assert!(result.columns.is_empty());
}

#[tokio::test]
async fn standalone_begin_is_noop() {
    let Some(c) = try_client() else { return };
    let result =
        dbx_core::db::turso_driver::execute_query(&c, "BEGIN").await.expect("standalone BEGIN should not fail");
    assert_eq!(result.affected_rows, 0);
}

#[tokio::test]
async fn error_on_invalid_sql() {
    let Some(c) = try_client() else { return };
    let result = dbx_core::db::turso_driver::execute_query(&c, "SELECT * FROM nonexistent_table_xyz").await;
    match result {
        Err(e) => assert!(e.contains("no such table"), "error should mention missing table: {e}"),
        Ok(_) => panic!("should have failed"),
    }
}
