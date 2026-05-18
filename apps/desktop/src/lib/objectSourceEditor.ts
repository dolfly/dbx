import type { DatabaseType, ObjectSourceKind } from "@/types/database";

type BuildEditableObjectSourceSqlInput = {
  databaseType: DatabaseType;
  objectType: ObjectSourceKind;
  schema?: string | null;
  name: string;
  source: string;
};

export type ObjectSourceSaveExecutionMode = "single" | "script";
export type ObjectSourceReadOnlyReason = "system-object";

type ObjectSourceReadOnlyInput = {
  databaseType: DatabaseType;
  schema?: string | null;
  name: string;
  objectType: ObjectSourceKind;
};

const postgresLikeRoutineRenameTypes = new Set<DatabaseType>([
  "postgres",
  "redshift",
  "gaussdb",
  "kingbase",
  "highgo",
  "vastbase",
]);

function quotePostgresIdentifier(value: string) {
  return `"${value.replaceAll('"', '""')}"`;
}

function ensureSemicolon(sql: string) {
  const trimmed = sql.trim();
  return trimmed.endsWith(";") ? trimmed : `${trimmed};`;
}

function postgresQualifiedName(schema: string | null | undefined, name: string) {
  return [schema, name]
    .filter(Boolean)
    .map((part) => quotePostgresIdentifier(part as string))
    .join(".");
}

function unquotePostgresIdentifier(value: string) {
  const trimmed = value.trim();
  if (trimmed.startsWith('"') && trimmed.endsWith('"')) return trimmed.slice(1, -1).replaceAll('""', '"');
  return trimmed;
}

function splitQualifiedRoutineName(value: string) {
  const parts = value.match(/"(?:""|[^"])+"|[A-Za-z_][\w$]*/g) ?? [];
  return parts.map(unquotePostgresIdentifier);
}

function routineDeclaration(source: string) {
  const match = source.match(
    /^\s*CREATE\s+(?:OR\s+REPLACE\s+)?(FUNCTION|PROCEDURE)\s+((?:"(?:""|[^"])+"|[A-Za-z_][\w$]*)(?:\s*\.\s*(?:"(?:""|[^"])+"|[A-Za-z_][\w$]*))?)\s*(\([^]*?\))?/i,
  );
  if (!match) return null;
  const nameParts = splitQualifiedRoutineName(match[2]);
  const name = nameParts[nameParts.length - 1];
  if (!name) return null;
  return {
    kind: match[1].toUpperCase() as "FUNCTION" | "PROCEDURE",
    name,
    signature: match[3]?.trim() ?? "",
  };
}

function routineNameChanged(sourceName: string, savedName: string) {
  return sourceName.toLowerCase() !== savedName.toLowerCase();
}

function buildRoutineRenameCleanup(input: BuildEditableObjectSourceSqlInput, source: string) {
  if (!postgresLikeRoutineRenameTypes.has(input.databaseType)) return null;
  if (input.objectType !== "FUNCTION" && input.objectType !== "PROCEDURE") return null;

  const declaration = routineDeclaration(source);
  if (!declaration || declaration.kind !== input.objectType) return null;
  if (!routineNameChanged(declaration.name, input.name)) return null;

  return `DROP ${input.objectType} IF EXISTS ${postgresQualifiedName(input.schema, input.name)}${declaration.signature};`;
}

export function buildExecutableObjectSourceStatements(input: BuildEditableObjectSourceSqlInput) {
  const source = input.source.trim();
  if (input.databaseType === "sqlserver") {
    return [source.replace(/^CREATE\s+(?:OR\s+ALTER\s+)?/i, "ALTER ")];
  }

  if ((input.databaseType === "postgres" || input.databaseType === "gaussdb") && input.objectType === "VIEW") {
    return [`CREATE OR REPLACE VIEW ${postgresQualifiedName(input.schema, input.name)} AS\n${ensureSemicolon(source)}`];
  }

  const createStatement = ensureSemicolon(source);
  const cleanup = buildRoutineRenameCleanup(input, source);
  return cleanup ? [createStatement, cleanup] : [createStatement];
}

export function buildExecutableObjectSourceSql(input: BuildEditableObjectSourceSqlInput) {
  return buildExecutableObjectSourceStatements(input).join("\n");
}

export function objectSourceSaveExecutionMode(_databaseType: DatabaseType): ObjectSourceSaveExecutionMode {
  return "single";
}

const readOnlySystemSchemas = new Set([
  "information_schema",
  "pg_catalog",
  "sys",
  "system",
  "mysql",
  "performance_schema",
  "xdb",
  "outln",
  "dbsnmp",
  "ctisys",
  "sysauditor",
  "syssso",
]);

const damengSysdbaRoutinePrefixes = ["SP_TS_", "SP_ARCH_", "SP_DB_", "SP_DROP_CONS_", "SP_TAB_", "SP_UPDATE_SYS"];

export function objectSourceReadOnlyReason(input: ObjectSourceReadOnlyInput): ObjectSourceReadOnlyReason | null {
  const schema = input.schema?.trim().toLowerCase();
  if (schema && readOnlySystemSchemas.has(schema)) return "system-object";

  const name = input.name.trim().toUpperCase();
  if (
    input.databaseType === "dameng" &&
    input.schema?.trim().toUpperCase() === "SYSDBA" &&
    (input.objectType === "PROCEDURE" || input.objectType === "FUNCTION") &&
    damengSysdbaRoutinePrefixes.some((prefix) => name.startsWith(prefix))
  ) {
    return "system-object";
  }

  return null;
}
