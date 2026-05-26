import { readFileSync } from "node:fs";
import assert from "node:assert/strict";
import test from "node:test";

test("Redis connection dialog exposes standalone and sentinel modes", () => {
  const source = readFileSync("apps/desktop/src/components/connection/ConnectionDialog.vue", "utf8");

  assert.match(source, /redis_connection_mode: "standalone"/);
  assert.match(source, /form\.redis_connection_mode === 'sentinel'/);
  assert.match(source, /t\("connection\.redisStandaloneMode"\)/);
  assert.match(source, /t\("connection\.redisSentinelMode"\)/);
  assert.match(source, /v-model="form\.redis_sentinel_nodes"/);
  assert.match(source, /v-model="form\.redis_sentinel_master"/);
  assert.match(source, /v-model="form\.redis_sentinel_username"/);
  assert.match(source, /v-model="form\.redis_sentinel_password"/);
  assert.match(source, /v-model="form\.redis_sentinel_tls"/);
});

test("Redis sentinel submit config normalizes nodes and uses the first sentinel as endpoint", () => {
  const source = readFileSync("apps/desktop/src/components/connection/ConnectionDialog.vue", "utf8");

  assert.match(source, /normalizeRedisSentinelNodes/);
  assert.match(source, /firstRedisSentinelEndpoint/);
  assert.match(source, /config\.host = firstNode\.host/);
  assert.match(source, /config\.port = firstNode\.port/);
  assert.match(source, /config\.redis_sentinel_master = config\.redis_sentinel_master\?\.trim\(\) \|\| ""/);
  assert.match(source, /config\.redis_connection_mode = "standalone"/);
});

test("Redis sentinel fields are typed and localized", () => {
  const typesSource = readFileSync("apps/desktop/src/types/database.ts", "utf8");
  const zhSource = readFileSync("apps/desktop/src/i18n/locales/zh-CN.ts", "utf8");
  const enSource = readFileSync("apps/desktop/src/i18n/locales/en.ts", "utf8");

  assert.match(typesSource, /redis_connection_mode\?: "standalone" \| "sentinel"/);
  assert.match(typesSource, /redis_sentinel_master\?: string/);
  assert.match(typesSource, /redis_sentinel_nodes\?: string/);
  assert.match(typesSource, /redis_sentinel_password\?: string/);
  assert.match(zhSource, /redisSentinelMode: "哨兵"/);
  assert.match(enSource, /redisSentinelMode: "Sentinel"/);
});

test("Tauri Redis connection commands route sentinel configs through the sentinel connector", () => {
  const source = readFileSync("src-tauri/src/commands/connection.rs", "utf8");

  assert.match(source, /config\.uses_redis_sentinel\(\)/);
  assert.match(source, /db_config\.uses_redis_sentinel\(\)/);
  assert.match(source, /db::redis_driver::connect_sentinel\(&config\)/);
  assert.match(source, /db::redis_driver::connect_sentinel\(&db_config\)/);
});
