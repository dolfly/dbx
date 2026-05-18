<script setup lang="ts">
import { computed, ref, watch } from "vue";
import { RecycleScroller } from "vue-virtual-scroller";
import {
  Braces,
  Code2,
  Copy,
  Eye,
  Loader2,
  Pencil,
  PencilLine,
  RefreshCw,
  Search,
  ScrollText,
  Table2,
  TerminalSquare,
  Trash2,
  X,
} from "lucide-vue-next";
import { useI18n } from "vue-i18n";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";
import {
  ContextMenu,
  ContextMenuContent,
  ContextMenuItem,
  ContextMenuSeparator,
  ContextMenuTrigger,
} from "@/components/ui/context-menu";
import DangerConfirmDialog from "@/components/editor/DangerConfirmDialog.vue";
import * as api from "@/lib/api";
import type { ConnectionConfig, ObjectInfo, ObjectSourceKind } from "@/types/database";
import { isSchemaAware } from "@/lib/databaseCapabilities";
import { buildTableSelectSql, qualifiedTableName } from "@/lib/tableSelectSql";
import { useToast } from "@/composables/useToast";
import {
  buildExecutableObjectSourceStatements,
  objectSourceReadOnlyReason,
  objectSourceSaveExecutionMode,
} from "@/lib/objectSourceEditor";
import { buildRenameObjectSql, supportsObjectRename } from "@/lib/objectRenameSql";
import { useConnectionStore } from "@/stores/connectionStore";
import { useQueryStore } from "@/stores/queryStore";
import QueryEditor from "@/components/editor/QueryEditor.vue";
import type { SqlFormatDialect } from "@/lib/sqlFormatter";
import { isCancelSearchShortcut } from "@/lib/keyboardShortcuts";
import { Dialog, DialogContent, DialogFooter, DialogHeader, DialogTitle } from "@/components/ui/dialog";
import { buildObjectBrowserRows, type ObjectBrowserRow } from "@/lib/objectBrowserRows";

type ObjectFilter = "all" | "tables" | "views" | "procedures" | "functions";

const props = defineProps<{
  connection: ConnectionConfig;
  database: string;
  schema?: string;
}>();

const emit = defineEmits<{
  openTable: [target: { tableName: string; schema?: string }];
  schemaChange: [schema: string | undefined];
}>();

const { t } = useI18n();
const { toast } = useToast();
const connectionStore = useConnectionStore();
const queryStore = useQueryStore();

const schemas = ref<string[]>([]);
const selectedSchema = ref<string | undefined>(props.schema);
const rows = ref<ObjectBrowserRow[]>([]);
const rootRef = ref<HTMLElement>();
const search = ref("");
const objectFilter = ref<ObjectFilter>("all");
const loadingSchemas = ref(false);
const loadingObjects = ref(false);
const sourceLoading = ref(false);
const sourceContent = ref("");
const sourceError = ref("");
const sourceRow = ref<ObjectBrowserRow | null>(null);
const sourceEditing = ref(false);
const sourceDraft = ref("");
const sourceSaving = ref(false);
const sourceSaveError = ref("");
const error = ref("");
const showDropConfirm = ref(false);
const dropTarget = ref<ObjectBrowserRow | null>(null);
const showRenameDialog = ref(false);
const renameTarget = ref<ObjectBrowserRow | null>(null);
const renameInput = ref("");
const renameError = ref("");
let loadId = 0;

const needsSchema = computed(() => isSchemaAware(props.connection.db_type));
const tableCount = computed(() => rows.value.filter((row) => row.type === "TABLE").length);
const viewCount = computed(() => rows.value.filter((row) => row.type === "VIEW").length);
const procedureCount = computed(() => rows.value.filter((row) => row.type === "PROCEDURE").length);
const functionCount = computed(() => rows.value.filter((row) => row.type === "FUNCTION").length);
const sourceDialect = computed<"mysql" | "postgres" | "sqlserver">(() => {
  if (props.connection.db_type === "postgres" || props.connection.db_type === "gaussdb") return "postgres";
  if (props.connection.db_type === "sqlserver") return "sqlserver";
  return "mysql";
});
const sourceFormatDialect = computed<SqlFormatDialect>(() => {
  switch (props.connection.db_type) {
    case "mysql":
    case "postgres":
    case "sqlite":
    case "sqlserver":
      return props.connection.db_type;
    default:
      return "generic";
  }
});
const sourceRowReadOnlyReason = computed(() => (sourceRow.value ? readOnlyReasonForRow(sourceRow.value) : null));
const objectFilters = computed<ObjectFilter[]>(() =>
  (
    [
      ["all", rows.value.length],
      ["tables", tableCount.value],
      ["views", viewCount.value],
      ["procedures", procedureCount.value],
      ["functions", functionCount.value],
    ] as Array<[ObjectFilter, number]>
  )
    .filter(([filter, count]) => filter === "all" || count > 0)
    .map(([filter]) => filter),
);
const showObjectFilter = computed(() => objectFilters.value.length > 2);
const hasComments = computed(() => rows.value.some((row) => row.comment?.trim()));
const gridTemplateColumns = computed(() =>
  hasComments.value ? "minmax(0,1fr) 120px 160px minmax(160px,0.7fr)" : "minmax(0,1fr) 120px 160px",
);
const searchedRows = computed(() => {
  const q = search.value.trim().toLowerCase();
  if (!q) return rows.value;
  return rows.value.filter((row) =>
    [row.name, row.schema, row.type, row.comment]
      .filter(Boolean)
      .some((value) => String(value).toLowerCase().includes(q)),
  );
});
const filteredRows = computed(() => {
  if (objectFilter.value === "tables") return searchedRows.value.filter((row) => row.type === "TABLE");
  if (objectFilter.value === "views") return searchedRows.value.filter((row) => row.type === "VIEW");
  if (objectFilter.value === "procedures") return searchedRows.value.filter((row) => row.type === "PROCEDURE");
  if (objectFilter.value === "functions") return searchedRows.value.filter((row) => row.type === "FUNCTION");
  return searchedRows.value;
});

function iconFor(row: ObjectBrowserRow) {
  if (row.type === "VIEW") return Eye;
  if (row.type === "PROCEDURE") return ScrollText;
  if (row.type === "FUNCTION") return Braces;
  return Table2;
}

function typeLabel(type: ObjectBrowserRow["type"]) {
  if (type === "VIEW") return t("objects.view");
  if (type === "PROCEDURE") return t("objects.procedure");
  if (type === "FUNCTION") return t("objects.function");
  return t("objects.table");
}

function iconClass(type: ObjectBrowserRow["type"]) {
  if (type === "VIEW") return "text-purple-500";
  if (type === "PROCEDURE") return "text-blue-500";
  if (type === "FUNCTION") return "text-amber-500";
  return "text-green-500";
}

function canOpenSource(row: ObjectBrowserRow) {
  return row.type === "VIEW" || row.type === "PROCEDURE" || row.type === "FUNCTION";
}

function canRename(row: ObjectBrowserRow) {
  return !readOnlyReasonForRow(row) && supportsObjectRename(props.connection.db_type, row.type);
}

function readOnlyReasonForRow(row: ObjectBrowserRow) {
  if (row.type !== "VIEW" && row.type !== "PROCEDURE" && row.type !== "FUNCTION") return null;
  return objectSourceReadOnlyReason({
    databaseType: props.connection.db_type,
    schema: row.schema || selectedSchema.value || props.database,
    name: row.name,
    objectType: row.type,
  });
}

function sourceTitle(row: ObjectBrowserRow | null) {
  if (!row) return t("objects.source");
  return `${row.name} ${t("objects.source")}`;
}

function openRow(row: ObjectBrowserRow) {
  if (row.type === "TABLE") {
    emit("openTable", { tableName: row.name, schema: row.schema });
    return;
  }
  if (canOpenSource(row)) {
    void openSource(row);
  }
}

async function openSource(row: ObjectBrowserRow) {
  sourceRow.value = row;
  sourceContent.value = "";
  sourceError.value = "";
  sourceEditing.value = false;
  sourceDraft.value = "";
  sourceSaveError.value = "";
  sourceLoading.value = true;
  try {
    const result = await api.getObjectSource(
      props.connection.id,
      props.database,
      row.schema || selectedSchema.value || props.database,
      row.name,
      row.type as ObjectSourceKind,
    );
    sourceContent.value = result.source;
    sourceDraft.value = result.source;
    sourceEditing.value = !readOnlyReasonForRow(row);
  } catch (e: any) {
    sourceError.value = e?.message || String(e);
  } finally {
    sourceLoading.value = false;
  }
}

function openNewQuery(row: ObjectBrowserRow) {
  const tabId = queryStore.createTab(props.connection.id, props.database, row.name);
  queryStore.updateSql(
    tabId,
    buildTableSelectSql({
      databaseType: props.connection.db_type,
      schema: row.schema || selectedSchema.value,
      tableName: row.name,
      limit: 100,
    }),
  );
}

function qualifiedName(row: ObjectBrowserRow): string {
  return qualifiedTableName({
    databaseType: props.connection.db_type,
    schema: row.schema || selectedSchema.value,
    tableName: row.name,
  });
}

function requestDrop(row: ObjectBrowserRow) {
  if (readOnlyReasonForRow(row)) {
    toast(t("objects.sourceReadOnlySystemObject"), 5000);
    return;
  }
  dropTarget.value = row;
  showDropConfirm.value = true;
}

function requestRename(row: ObjectBrowserRow) {
  if (readOnlyReasonForRow(row)) {
    toast(t("objects.sourceReadOnlySystemObject"), 5000);
    return;
  }
  renameTarget.value = row;
  renameInput.value = row.name;
  renameError.value = "";
  showRenameDialog.value = true;
}

function renamePreviewSql() {
  const row = renameTarget.value;
  const newName = renameInput.value.trim();
  if (!row || !newName || newName === row.name) return "";
  try {
    return buildRenameObjectSql({
      databaseType: props.connection.db_type,
      objectType: row.type,
      schema: row.schema || selectedSchema.value,
      oldName: row.name,
      newName,
    });
  } catch {
    return "";
  }
}

async function confirmRename() {
  const row = renameTarget.value;
  const newName = renameInput.value.trim();
  if (!row || !newName || newName === row.name) return;
  if (readOnlyReasonForRow(row)) {
    renameError.value = t("objects.sourceReadOnlySystemObject");
    return;
  }
  renameError.value = "";
  try {
    const schema = row.schema || selectedSchema.value || props.database;
    const sql = buildRenameObjectSql({
      databaseType: props.connection.db_type,
      objectType: row.type,
      schema,
      oldName: row.name,
      newName,
    });
    await api.executeQuery(props.connection.id, props.database, sql, schema);
    toast(t("contextMenu.renameObjectSuccess", { oldName: row.name, newName }));
    showRenameDialog.value = false;
    if (sourceRow.value?.id === row.id) closeSource();
    await reload();
    await connectionStore.refreshObjectListTreeNode(
      props.connection.id,
      props.database,
      row.schema || selectedSchema.value,
    );
  } catch (e: any) {
    renameError.value = e?.message || String(e);
  }
}

async function confirmDrop() {
  if (!dropTarget.value) return;
  const row = dropTarget.value;
  if (readOnlyReasonForRow(row)) {
    toast(t("objects.sourceReadOnlySystemObject"), 5000);
    dropTarget.value = null;
    return;
  }
  const typeSql =
    row.type === "VIEW"
      ? "VIEW"
      : row.type === "PROCEDURE"
        ? "PROCEDURE"
        : row.type === "FUNCTION"
          ? "FUNCTION"
          : "TABLE";
  const name = qualifiedName(row);
  try {
    await api.executeQuery(props.connection.id, props.database, `DROP ${typeSql} ${name}`);
    const successKey =
      row.type === "VIEW"
        ? "contextMenu.dropViewSuccess"
        : row.type === "PROCEDURE"
          ? "contextMenu.dropProcedureSuccess"
          : row.type === "FUNCTION"
            ? "contextMenu.dropFunctionSuccess"
            : "contextMenu.dropTableSuccess";
    toast(t(successKey, { name: row.name }));
    await reload();
    await connectionStore.refreshObjectListTreeNode(
      props.connection.id,
      props.database,
      row.schema || selectedSchema.value,
    );
  } catch (e: any) {
    toast(t("contextMenu.tableOperationFailed", { message: e?.message || String(e) }), 5000);
  }
  dropTarget.value = null;
}

function dropConfirmTitle(): string {
  if (!dropTarget.value) return "";
  const type = dropTarget.value.type;
  if (type === "VIEW") return t("contextMenu.confirmDropViewTitle");
  if (type === "PROCEDURE") return t("contextMenu.confirmDropProcedureTitle");
  if (type === "FUNCTION") return t("contextMenu.confirmDropFunctionTitle");
  return t("contextMenu.confirmDropTableTitle");
}

function dropConfirmMessage(): string {
  if (!dropTarget.value) return "";
  const name = dropTarget.value.name;
  const type = dropTarget.value.type;
  if (type === "VIEW") return t("contextMenu.confirmDropViewMessage", { name });
  if (type === "PROCEDURE") return t("contextMenu.confirmDropProcedureMessage", { name });
  if (type === "FUNCTION") return t("contextMenu.confirmDropFunctionMessage", { name });
  return t("contextMenu.confirmDropTableMessage", { name });
}

function closeSource() {
  sourceRow.value = null;
  sourceContent.value = "";
  sourceError.value = "";
  sourceEditing.value = false;
  sourceDraft.value = "";
  sourceSaveError.value = "";
}

function copySource() {
  if (!sourceContent.value) return;
  navigator.clipboard.writeText(sourceContent.value);
  toast(t("grid.copied"));
}

function editSource() {
  if (!sourceRow.value || !sourceContent.value) return;
  if (sourceRowReadOnlyReason.value) {
    toast(t("objects.sourceReadOnlySystemObject"), 5000);
    return;
  }
  sourceDraft.value = sourceContent.value;
  sourceSaveError.value = "";
  sourceEditing.value = true;
}

function cancelEditSource() {
  sourceEditing.value = false;
  sourceDraft.value = "";
  sourceSaveError.value = "";
}

async function saveSource() {
  if (!sourceRow.value || !sourceDraft.value.trim()) return;
  if (sourceRowReadOnlyReason.value) {
    toast(t("objects.sourceReadOnlySystemObject"), 5000);
    return;
  }
  const row = sourceRow.value;
  const schema = row.schema || selectedSchema.value || props.database;
  sourceSaving.value = true;
  sourceSaveError.value = "";
  try {
    const statements = buildExecutableObjectSourceStatements({
      databaseType: props.connection.db_type,
      objectType: row.type as ObjectSourceKind,
      schema,
      name: row.name,
      source: sourceDraft.value,
    });
    for (const sql of statements) {
      if (objectSourceSaveExecutionMode(props.connection.db_type) === "single") {
        await api.executeQuery(props.connection.id, props.database, sql, schema);
      } else {
        await api.executeScript(props.connection.id, props.database, sql, schema);
      }
    }
    toast(t("objects.sourceSaved"));
    sourceEditing.value = false;
    sourceDraft.value = "";
    await openSource(row);
  } catch (e: any) {
    sourceSaveError.value = e?.message || String(e);
  } finally {
    sourceSaving.value = false;
  }
}

async function loadSchemas() {
  if (!needsSchema.value) {
    schemas.value = [];
    selectedSchema.value = undefined;
    return;
  }
  loadingSchemas.value = true;
  try {
    const names = await api.listSchemas(props.connection.id, props.database);
    schemas.value = names;
    if (!selectedSchema.value || !names.includes(selectedSchema.value)) {
      selectedSchema.value = names.includes("public") ? "public" : names[0];
    }
  } finally {
    loadingSchemas.value = false;
  }
}

async function loadObjects() {
  const id = ++loadId;
  loadingObjects.value = true;
  error.value = "";
  rows.value = [];
  try {
    const schema = needsSchema.value ? selectedSchema.value || "" : props.database;
    const objects: ObjectInfo[] = await api.listObjects(props.connection.id, props.database, schema);
    if (id !== loadId) return;
    rows.value = buildObjectBrowserRows({
      objects,
      database: props.database,
      fallbackSchema: schema,
      needsSchema: needsSchema.value,
    });
  } catch (e: any) {
    if (id !== loadId) return;
    error.value = e?.message || String(e);
  } finally {
    if (id === loadId) loadingObjects.value = false;
  }
}

async function reload() {
  await loadSchemas();
  await loadObjects();
}

function onSchemaChange(value: any) {
  selectedSchema.value = typeof value === "string" && value ? value : undefined;
  emit("schemaChange", selectedSchema.value);
  void loadObjects();
}

function filterCount(filter: ObjectFilter) {
  if (filter === "tables") return tableCount.value;
  if (filter === "views") return viewCount.value;
  if (filter === "procedures") return procedureCount.value;
  if (filter === "functions") return functionCount.value;
  return rows.value.length;
}

function filterLabel(filter: ObjectFilter) {
  const key =
    filter === "tables"
      ? "objects.tables"
      : filter === "views"
        ? "objects.views"
        : filter === "procedures"
          ? "objects.procedures"
          : filter === "functions"
            ? "objects.functions"
            : "objects.all";
  return `${t(key)} ${filterCount(filter)}`;
}

function getSearchInput(): HTMLInputElement | null {
  return rootRef.value?.querySelector<HTMLInputElement>("[data-object-search-input]") ?? null;
}

function focusSearch(): boolean {
  const input = getSearchInput();
  if (!input) return false;
  input.focus();
  input.select();
  return true;
}

function onSearchKeydown(event: KeyboardEvent) {
  if (!isCancelSearchShortcut(event)) return;
  event.preventDefault();
  search.value = "";
}

defineExpose({ focusSearch });

watch(
  () => [props.connection.id, props.database, props.schema] as const,
  () => {
    selectedSchema.value = props.schema;
    void reload();
  },
  { immediate: true },
);
</script>

<template>
  <div ref="rootRef" class="flex h-full min-h-0 flex-col bg-background">
    <div class="flex h-10 shrink-0 items-center gap-2 border-b px-3">
      <div class="flex min-w-0 flex-1 items-center gap-2">
        <Table2 class="h-4 w-4 text-muted-foreground" />
        <div class="min-w-0 truncate text-sm font-medium">
          {{ props.database }}<template v-if="selectedSchema"> / {{ selectedSchema }}</template>
        </div>
        <div class="shrink-0 rounded border bg-muted/40 px-1.5 py-0.5 text-xs text-muted-foreground">
          {{ filteredRows.length }} / {{ rows.length }}
        </div>
      </div>
      <div class="flex min-w-[240px] flex-1 items-center gap-2">
        <Search class="h-3.5 w-3.5 shrink-0 text-muted-foreground" />
        <Input
          v-model="search"
          data-object-search-input
          class="h-7 text-xs"
          :placeholder="t('objects.search')"
          @keydown="onSearchKeydown"
        />
        <div v-if="showObjectFilter" class="flex h-7 shrink-0 items-center rounded border bg-muted/20 p-0.5">
          <button
            v-for="filter in objectFilters"
            :key="filter"
            type="button"
            class="h-6 rounded-sm px-2 text-xs text-muted-foreground transition-colors hover:text-foreground"
            :class="{ 'bg-background text-foreground shadow-sm': objectFilter === filter }"
            @click="objectFilter = filter"
          >
            {{ filterLabel(filter) }}
          </button>
        </div>
      </div>
      <Select
        v-if="needsSchema"
        :model-value="selectedSchema"
        :disabled="loadingSchemas"
        @update:model-value="onSchemaChange"
      >
        <SelectTrigger class="h-7 w-36 text-xs">
          <SelectValue :placeholder="loadingSchemas ? t('objects.loadingSchemas') : t('objects.schema')" />
        </SelectTrigger>
        <SelectContent>
          <SelectItem v-for="schema in schemas" :key="schema" :value="schema">{{ schema }}</SelectItem>
        </SelectContent>
      </Select>
      <Button variant="ghost" size="icon" class="h-7 w-7" :disabled="loadingObjects" @click="reload">
        <RefreshCw class="h-3.5 w-3.5" :class="{ 'animate-spin': loadingObjects }" />
      </Button>
    </div>

    <div v-if="loadingObjects" class="flex flex-1 items-center justify-center gap-2 text-sm text-muted-foreground">
      <Loader2 class="h-4 w-4 animate-spin" />
      {{ t("objects.loading") }}
    </div>
    <div v-else-if="error" class="flex flex-1 items-center justify-center px-6 text-center text-sm text-destructive">
      {{ error }}
    </div>
    <div
      v-else-if="filteredRows.length === 0"
      class="flex flex-1 items-center justify-center text-sm text-muted-foreground"
    >
      {{ t("objects.empty") }}
    </div>
    <div v-else class="flex min-h-0 flex-1 flex-col">
      <div
        class="grid h-8 shrink-0 items-center gap-3 border-b bg-muted/40 px-3 text-xs font-medium text-muted-foreground"
        :style="{ gridTemplateColumns }"
      >
        <div class="truncate">{{ t("objects.name") }}</div>
        <div class="truncate">{{ t("objects.type") }}</div>
        <div class="truncate">{{ t("objects.schemaColumn") }}</div>
        <div v-if="hasComments" class="truncate">{{ t("objects.comment") }}</div>
      </div>
      <RecycleScroller
        class="object-browser-scroller min-h-0 flex-1"
        :items="filteredRows"
        :item-size="38"
        :buffer="600"
        :skip-hover="true"
        key-field="id"
      >
        <template #default="{ item }">
          <ContextMenu>
            <ContextMenuTrigger as-child>
              <div
                class="grid h-[38px] cursor-pointer items-center gap-3 border-b px-3 hover:bg-accent/50"
                :class="{ 'bg-accent/40': sourceRow?.id === item.id }"
                :style="{ gridTemplateColumns }"
                @click="openRow(item)"
              >
                <div class="flex min-w-0 items-center gap-2">
                  <component :is="iconFor(item)" class="h-3.5 w-3.5 shrink-0" :class="iconClass(item.type)" />
                  <span class="truncate text-[13px] font-medium text-foreground">{{ item.name }}</span>
                </div>
                <div class="truncate text-xs text-muted-foreground">{{ typeLabel(item.type) }}</div>
                <div class="truncate text-xs text-muted-foreground">{{ item.schema || props.database }}</div>
                <div v-if="hasComments" class="truncate text-xs text-muted-foreground" :title="item.comment || ''">
                  {{ item.comment || "" }}
                </div>
              </div>
            </ContextMenuTrigger>
            <ContextMenuContent class="w-auto min-w-40">
              <!-- TABLE -->
              <template v-if="item.type === 'TABLE'">
                <ContextMenuItem @click="openRow(item)">
                  <Table2 class="w-4 h-4 mr-2" /> {{ t("contextMenu.viewData") }}
                </ContextMenuItem>
                <ContextMenuItem @click="openNewQuery(item)">
                  <TerminalSquare class="w-4 h-4 mr-2" /> {{ t("contextMenu.newQuery") }}
                </ContextMenuItem>
                <ContextMenuItem v-if="canRename(item)" @click="requestRename(item)">
                  <Pencil class="w-4 h-4 mr-2" /> {{ t("contextMenu.renameObject") }}
                </ContextMenuItem>
                <ContextMenuSeparator />
                <ContextMenuItem class="text-destructive" @click="requestDrop(item)">
                  <Trash2 class="w-4 h-4 mr-2" /> {{ t("contextMenu.dropTable") }}
                </ContextMenuItem>
              </template>
              <!-- VIEW -->
              <template v-else-if="item.type === 'VIEW'">
                <ContextMenuItem @click="openSource(item)">
                  <Code2 class="w-4 h-4 mr-2" /> {{ t("contextMenu.viewSource") }}
                </ContextMenuItem>
                <ContextMenuItem v-if="canRename(item)" @click="requestRename(item)">
                  <Pencil class="w-4 h-4 mr-2" /> {{ t("contextMenu.renameObject") }}
                </ContextMenuItem>
                <ContextMenuSeparator />
                <ContextMenuItem class="text-destructive" @click="requestDrop(item)">
                  <Trash2 class="w-4 h-4 mr-2" /> {{ t("contextMenu.dropView") }}
                </ContextMenuItem>
              </template>
              <!-- PROCEDURE / FUNCTION -->
              <template v-else>
                <ContextMenuItem @click="openSource(item)">
                  <Code2 class="w-4 h-4 mr-2" /> {{ t("contextMenu.viewSource") }}
                </ContextMenuItem>
                <ContextMenuItem v-if="canRename(item)" @click="requestRename(item)">
                  <Pencil class="w-4 h-4 mr-2" /> {{ t("contextMenu.renameObject") }}
                </ContextMenuItem>
                <template v-if="!readOnlyReasonForRow(item)">
                  <ContextMenuSeparator />
                  <ContextMenuItem class="text-destructive" @click="requestDrop(item)">
                    <Trash2 class="w-4 h-4 mr-2" />
                    {{ item.type === "PROCEDURE" ? t("contextMenu.dropProcedure") : t("contextMenu.dropFunction") }}
                  </ContextMenuItem>
                </template>
              </template>
            </ContextMenuContent>
          </ContextMenu>
        </template>
      </RecycleScroller>
      <div v-if="sourceRow" class="flex h-[42%] min-h-44 shrink-0 flex-col border-t bg-background">
        <div class="flex h-8 shrink-0 items-center gap-2 border-b bg-muted/20 px-3">
          <Code2 class="h-3.5 w-3.5 text-muted-foreground" />
          <span class="min-w-0 flex-1 truncate text-xs font-medium">{{ sourceTitle(sourceRow) }}</span>
          <Button
            v-if="sourceEditing"
            variant="ghost"
            size="sm"
            class="h-6 px-2 text-xs"
            :disabled="sourceSaving || !sourceDraft.trim()"
            @click="saveSource"
          >
            <Loader2 v-if="sourceSaving" class="mr-1 h-3 w-3 animate-spin" />
            {{ t("objects.saveSource") }}
          </Button>
          <Button
            v-if="sourceEditing"
            variant="ghost"
            size="sm"
            class="h-6 px-2 text-xs"
            :disabled="sourceSaving"
            @click="cancelEditSource"
          >
            {{ t("objects.cancelEdit") }}
          </Button>
          <Button
            v-if="!sourceEditing"
            variant="ghost"
            size="icon"
            class="h-5 w-5"
            :disabled="!sourceContent"
            @click="copySource"
          >
            <Copy class="h-3 w-3" />
          </Button>
          <Button
            v-if="!sourceEditing"
            variant="ghost"
            size="icon"
            class="h-5 w-5"
            :disabled="!sourceContent || !!sourceRowReadOnlyReason"
            :title="sourceRowReadOnlyReason ? t('objects.sourceReadOnlySystemObject') : undefined"
            @click="editSource"
          >
            <PencilLine class="h-3 w-3" />
          </Button>
          <Button variant="ghost" size="icon" class="h-5 w-5" @click="closeSource">
            <X class="h-3 w-3" />
          </Button>
        </div>
        <div v-if="sourceLoading" class="flex flex-1 items-center justify-center">
          <Loader2 class="h-4 w-4 animate-spin text-muted-foreground" />
        </div>
        <div v-else-if="sourceError" class="flex flex-1 items-center justify-center px-4 text-sm text-destructive">
          {{ sourceError }}
        </div>
        <div v-else-if="sourceEditing" class="flex min-h-0 flex-1 flex-col" data-object-source-editor>
          <QueryEditor
            v-model="sourceDraft"
            class="min-h-0 flex-1"
            :connection-id="props.connection.id"
            :database="props.database"
            :dialect="sourceDialect"
            :format-dialect="sourceFormatDialect"
            :read-only="!!sourceRowReadOnlyReason"
            force-word-wrap
            @save="saveSource"
          />
          <div v-if="sourceSaveError" class="shrink-0 border-t px-3 py-2 text-xs text-destructive">
            {{ sourceSaveError }}
          </div>
        </div>
        <QueryEditor
          v-else
          :key="`source-preview-${sourceRow.id}`"
          :model-value="sourceContent"
          class="min-h-0 flex-1"
          :connection-id="props.connection.id"
          :database="props.database"
          :dialect="sourceDialect"
          :format-dialect="sourceFormatDialect"
          force-word-wrap
          read-only
          data-object-source-preview
        />
      </div>
    </div>
  </div>

  <DangerConfirmDialog
    v-model:open="showDropConfirm"
    :title="dropConfirmTitle()"
    :details="dropConfirmMessage()"
    :confirm-label="t('dangerDialog.deleteConfirm')"
    @confirm="confirmDrop"
  />

  <Dialog v-model:open="showRenameDialog">
    <DialogContent class="sm:max-w-[420px]">
      <DialogHeader>
        <DialogTitle>{{ t("contextMenu.renameObjectTitle") }}</DialogTitle>
      </DialogHeader>
      <div class="grid gap-3">
        <Input
          v-model="renameInput"
          :placeholder="t('contextMenu.renameObjectNamePlaceholder')"
          @keydown.enter.prevent="confirmRename"
        />
        <pre
          v-if="renamePreviewSql()"
          class="max-h-32 overflow-auto rounded bg-muted p-3 text-xs whitespace-pre-wrap"
          >{{ renamePreviewSql() }}</pre
        >
        <p v-if="renameError" class="text-sm text-destructive">{{ renameError }}</p>
      </div>
      <DialogFooter>
        <Button variant="outline" @click="showRenameDialog = false">{{ t("dangerDialog.cancel") }}</Button>
        <Button :disabled="!renameInput.trim() || renameInput.trim() === renameTarget?.name" @click="confirmRename">
          {{ t("contextMenu.renameObject") }}
        </Button>
      </DialogFooter>
    </DialogContent>
  </Dialog>
</template>

<style scoped>
.object-browser-scroller {
  will-change: scroll-position;
  contain: content;
}

.object-browser-scroller :deep(.vue-recycle-scroller__item-view) {
  contain: layout style paint;
}
</style>
