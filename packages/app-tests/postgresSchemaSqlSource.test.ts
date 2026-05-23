import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";

const source = readFileSync("crates/dbx-core/src/db/postgres.rs", "utf8");

test("postgres table list SQL has no trailing comma before FROM", () => {
  assert.doesNotMatch(source, /AS table_comment,\s*\\\s*FROM pg_catalog\.pg_class/);
  assert.match(source, /obj_description\(c\.oid\) AS table_comment\s*\\\s*FROM pg_catalog\.pg_class/);
});
