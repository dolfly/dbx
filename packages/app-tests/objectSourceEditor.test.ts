import { strict as assert } from "node:assert";
import test from "node:test";
import {
  buildExecutableObjectSourceSql,
  buildExecutableObjectSourceStatements,
  objectSourceReadOnlyReason,
  objectSourceSaveExecutionMode,
} from "../../apps/desktop/src/lib/objectSourceEditor.ts";

test("SQL Server edited source saves as ALTER", () => {
  const sql = buildExecutableObjectSourceSql({
    databaseType: "sqlserver",
    objectType: "PROCEDURE",
    schema: "dbo",
    name: "usp_demo",
    source: "CREATE PROCEDURE dbo.usp_demo AS SELECT 1;",
  });

  assert.equal(sql, "ALTER PROCEDURE dbo.usp_demo AS SELECT 1;");
});

test("SQL Server edited CREATE OR ALTER source saves as ALTER", () => {
  const sql = buildExecutableObjectSourceSql({
    databaseType: "sqlserver",
    objectType: "VIEW",
    schema: "dbo",
    name: "vw_demo",
    source: "CREATE OR ALTER VIEW dbo.vw_demo AS SELECT 1 AS id;",
  });

  assert.equal(sql, "ALTER VIEW dbo.vw_demo AS SELECT 1 AS id;");
});

test("SQL Server object source saves as a single batch", () => {
  assert.equal(objectSourceSaveExecutionMode("sqlserver"), "single");
});

test("Kingbase object source saves as a single statement", () => {
  assert.equal(objectSourceSaveExecutionMode("kingbase"), "single");
});

test("Postgres-family object source saves as a single statement", () => {
  assert.equal(objectSourceSaveExecutionMode("postgres"), "single");
  assert.equal(objectSourceSaveExecutionMode("gaussdb"), "single");
});

test("MySQL object source saves as a single statement", () => {
  assert.equal(objectSourceSaveExecutionMode("mysql"), "single");
});

test("Postgres view body opens as CREATE OR REPLACE VIEW", () => {
  const sql = buildExecutableObjectSourceSql({
    databaseType: "postgres",
    objectType: "VIEW",
    schema: "public",
    name: "active users",
    source: " SELECT id, name FROM users WHERE active ",
  });

  assert.equal(sql, 'CREATE OR REPLACE VIEW "public"."active users" AS\nSELECT id, name FROM users WHERE active;');
});

test("Kingbase function rename creates the renamed routine and then drops the original routine", () => {
  const statements = buildExecutableObjectSourceStatements({
    databaseType: "kingbase",
    objectType: "FUNCTION",
    schema: "DLJPM",
    name: "CONVERTSPECIALNAME",
    source:
      'CREATE OR REPLACE function "DLJPM"."CONVERTSPECIALNAME1" (SpName varchar2)\nRETURN VARCHAR2\nas\nbegin\nreturn SpName;\nend;',
  });

  assert.deepEqual(statements, [
    'CREATE OR REPLACE function "DLJPM"."CONVERTSPECIALNAME1" (SpName varchar2)\nRETURN VARCHAR2\nas\nbegin\nreturn SpName;\nend;',
    'DROP FUNCTION IF EXISTS "DLJPM"."CONVERTSPECIALNAME"(SpName varchar2);',
  ]);
});

test("Postgres procedure rename creates the renamed routine and then drops the original routine", () => {
  const statements = buildExecutableObjectSourceStatements({
    databaseType: "postgres",
    objectType: "PROCEDURE",
    schema: "public",
    name: "refresh_cache",
    source: 'CREATE OR REPLACE PROCEDURE "public"."refresh_cache_v2"(mode text)\nLANGUAGE SQL\nAS $$ SELECT 1 $$;',
  });

  assert.deepEqual(statements, [
    'CREATE OR REPLACE PROCEDURE "public"."refresh_cache_v2"(mode text)\nLANGUAGE SQL\nAS $$ SELECT 1 $$;',
    'DROP PROCEDURE IF EXISTS "public"."refresh_cache"(mode text);',
  ]);
});

test("object source SQL joins generated save statements for previews", () => {
  const sql = buildExecutableObjectSourceSql({
    databaseType: "postgres",
    objectType: "PROCEDURE",
    schema: "public",
    name: "refresh_cache",
    source: 'CREATE OR REPLACE PROCEDURE "public"."refresh_cache_v2"(mode text)\nLANGUAGE SQL\nAS $$ SELECT 1 $$;',
  });

  assert.equal(
    sql,
    'CREATE OR REPLACE PROCEDURE "public"."refresh_cache_v2"(mode text)\nLANGUAGE SQL\nAS $$ SELECT 1 $$;\nDROP PROCEDURE IF EXISTS "public"."refresh_cache"(mode text);',
  );
});

test("system schemas open object source as read-only", () => {
  assert.equal(
    objectSourceReadOnlyReason({
      databaseType: "oracle",
      schema: "SYS",
      name: "DBMS_STATS",
      objectType: "PROCEDURE",
    }),
    "system-object",
  );
});

test("Dameng built-in maintenance routines open as read-only without blocking user SP names", () => {
  assert.equal(
    objectSourceReadOnlyReason({
      databaseType: "dameng",
      schema: "SYSDBA",
      name: "SP_TS_BAKSET_REMOVE_BATCH",
      objectType: "PROCEDURE",
    }),
    "system-object",
  );

  assert.equal(
    objectSourceReadOnlyReason({
      databaseType: "dameng",
      schema: "SYSDBA",
      name: "SP_HELLO",
      objectType: "PROCEDURE",
    }),
    null,
  );
});
