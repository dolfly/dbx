<script setup lang="ts">
import { ref, computed, nextTick, watch } from "vue";
import { useSqlHighlighter } from "@/composables/useSqlHighlighter";
import { useI18n } from "vue-i18n";
import { translateBackendError } from "@/i18n/backend-errors";
import {
  Database,
  Table,
  Columns3,
  Eye,
  ChevronRight,
  ChevronDown,
  Loader2,
  FolderOpen,
  FolderClosed,
  Trash2,
  TerminalSquare,
  RefreshCw,
  Copy,
  TableProperties,
  Key,
  Link,
  Zap,
  ListTree,
  Pencil,
  Plug,
  Unplug,
  Pin,
  ArrowRightLeft,
  Download,
  FileCode,
  Network,
  FileUp,
  PencilRuler,
  Search,
  FolderInput,
  FolderPlus,
  Eraser,
  Scissors,
  CopyPlus,
  Plus,
  FileText,
  ScrollText,
  Braces,
  Code2,
  ListFilter,
} from "lucide-vue-next";
import {
  ContextMenu,
  ContextMenuContent,
  ContextMenuItem,
  ContextMenuSeparator,
  ContextMenuTrigger,
  ContextMenuSub,
  ContextMenuSubTrigger,
  ContextMenuSubContent,
} from "@/components/ui/context-menu";
import { useConnectionStore } from "@/stores/connectionStore";
import { useQueryStore } from "@/stores/queryStore";
import { useSavedSqlStore } from "@/stores/savedSqlStore";
import { useSettingsStore } from "@/stores/settingsStore";
import { useToast } from "@/composables/useToast";
import { useDatabaseOptions } from "@/composables/useDatabaseOptions";
import type { DatabaseType, TreeNode, TreeNodeType } from "@/types/database";
import * as api from "@/lib/api";
import { uuid } from "@/lib/utils";
import { resolveDefaultDatabase } from "@/lib/defaultDatabase";
import { canTreeNodeShowExpander, treeItemPaddingLeft } from "@/lib/sidebarTreeItemLayout";
import { buildTableSelectSql } from "@/lib/tableSelectSql";
import { editablePrimaryKeys, usesSyntheticRowIdKey } from "@/lib/tableEditing";
import {
  supportsDatabaseCreation,
  supportsDatabaseSearch,
  supportsFieldLineage,
  supportsObjectBrowserTreeNode,
  supportsSchemaDiagram,
  supportsSqlFileExecution,
  supportsTableImport,
  supportsTableTruncate,
  supportsTableStructureEditing,
  usesTreeSchemaMode,
} from "@/lib/databaseCapabilities";
import {
  objectSourceKindForTreeNode,
  sidebarSelectionCopyAction,
  treeNodeRowAction,
  treeNodeRowDoubleClickAction,
} from "@/lib/treeNodeClick";
import { formatSqlInsert } from "@/lib/exportFormats";
import { fetchTableDataForExport } from "@/lib/tableDataExport";
import {
  buildCreateDatabaseSql,
  buildDuckDbAttachDatabaseSql,
  duckDbAttachedDatabaseNameFromPath,
  supportsCreateDatabaseCharset,
  uniqueDuckDbAttachedDatabaseName,
} from "@/lib/createDatabaseSql";
import {
  buildCreateSchemaSql,
  buildDropDatabaseSql,
  buildDropObjectSql,
  buildDropSchemaSql,
  buildDropTableSql,
  buildDuplicateTableStructureSql,
  buildEmptyTableSql,
  buildTruncateTableSql,
  type DropObjectSqlOptions,
  type TableAdminSqlOptions,
} from "@/lib/dbAdminSql";
import { buildRenameObjectSql, supportsObjectRename, type RenameableObjectType } from "@/lib/objectRenameSql";
import { buildRoutineRenameObjectSourceStatements, supportsSourceBackedRoutineRename } from "@/lib/objectSourceEditor";
import { buildViewDdl } from "@/lib/viewDdl";
import { hexToRgba } from "@/lib/color";
import { focusSidebarRenameInput, shouldPreventRenameCloseAutoFocus } from "@/lib/sidebarRenameFocus";
import { hasTreeNodeDatabaseContext } from "@/lib/treeNodeContext";
import { sidebarDisplayTableName } from "@/lib/sidebarTableNameDisplay";
import DangerConfirmDialog from "@/components/editor/DangerConfirmDialog.vue";
import { isTauriRuntime } from "@/lib/tauriRuntime";
import { copyToClipboard } from "@/lib/clipboard";
import DatabaseIcon from "@/components/icons/DatabaseIcon.vue";
import ConnectionErrorIndicator from "@/components/connection/ConnectionErrorIndicator.vue";
import VisibleDatabasesDialog from "@/components/sidebar/VisibleDatabasesDialog.vue";
import { Dialog, DialogContent, DialogHeader, DialogTitle, DialogFooter } from "@/components/ui/dialog";
import { Button } from "@/components/ui/button";
import { Badge } from "@/components/ui/badge";
import { Input } from "@/components/ui/input";
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip";

const { t } = useI18n();
const labelRef = ref<HTMLElement>();
const rowRef = ref<HTMLElement>();
const isTruncated = computed(() => {
  const el = labelRef.value;
  return !!el && el.scrollWidth > el.clientWidth;
});
const connectionStore = useConnectionStore();
const queryStore = useQueryStore();
const savedSqlStore = useSavedSqlStore();
const settingsStore = useSettingsStore();
const { toast } = useToast();
const { highlight } = useSqlHighlighter();
const { getDatabaseOptions } = useDatabaseOptions();
const showVisibleDatabasesDialog = ref(false);

const props = defineProps<{
  node: TreeNode;
  depth: number;
  dragDisabled?: boolean;
  pendingRename?: boolean;
}>();

const emit = defineEmits<{
  "rename-started": [];
  "node-toggled": [node: TreeNode, wasExpanded: boolean];
  "search-toggle": [node: TreeNode];
}>();

function currentDatabaseType(): DatabaseType | undefined {
  return props.node.connectionId ? connectionStore.getConfig(props.node.connectionId)?.db_type : undefined;
}

function hasNodeDatabaseContext(node: TreeNode): node is TreeNode & { connectionId: string; database: string } {
  return !!node.connectionId && hasTreeNodeDatabaseContext(node);
}

function getIconInfo(node: TreeNode): { icon: any; colorClass: string } | null {
  switch (node.type) {
    case "connection":
      return null;
    case "connection-group":
      return { icon: node.isExpanded ? FolderOpen : FolderClosed, colorClass: "text-amber-500" };
    case "database":
      return { icon: Database, colorClass: "text-yellow-500" };
    case "schema":
      return { icon: FolderOpen, colorClass: "text-sky-400" };
    case "table":
      return { icon: Table, colorClass: "text-green-500" };
    case "view":
      return { icon: Eye, colorClass: "text-purple-500" };
    case "column":
      return { icon: Columns3, colorClass: "text-muted-foreground" };
    case "group-columns":
      return { icon: ListTree, colorClass: "text-green-400" };
    case "group-indexes":
      return { icon: Key, colorClass: "text-amber-500" };
    case "group-fkeys":
      return { icon: Link, colorClass: "text-blue-400" };
    case "group-triggers":
      return { icon: Zap, colorClass: "text-orange-400" };
    case "object-browser":
      return { icon: TableProperties, colorClass: "text-primary" };
    case "saved-sql-root":
      return { icon: FolderOpen, colorClass: "text-blue-500" };
    case "saved-sql-folder":
      return { icon: node.isExpanded ? FolderOpen : FolderClosed, colorClass: "text-blue-400" };
    case "saved-sql-file":
      return { icon: FileText, colorClass: "text-blue-300" };
    case "index":
      return { icon: Key, colorClass: "text-amber-400" };
    case "fkey":
      return { icon: Link, colorClass: "text-blue-300" };
    case "trigger":
      return { icon: Zap, colorClass: "text-orange-300" };
    case "redis-db":
      return { icon: Database, colorClass: "text-red-400" };
    case "mongo-db":
      return { icon: Database, colorClass: "text-yellow-500" };
    case "mongo-collection":
      return { icon: Table, colorClass: "text-green-400" };
    case "procedure":
      return { icon: ScrollText, colorClass: "text-blue-500" };
    case "function":
      return { icon: Braces, colorClass: "text-amber-500" };
    case "group-tables":
      return { icon: Table, colorClass: "text-green-500" };
    case "group-views":
      return { icon: Eye, colorClass: "text-purple-500" };
    case "group-procedures":
      return { icon: ScrollText, colorClass: "text-blue-500" };
    case "group-functions":
      return { icon: Braces, colorClass: "text-amber-500" };
    default:
      return { icon: Database, colorClass: "text-muted-foreground" };
  }
}

const groupTypes: Set<TreeNodeType> = new Set([
  "group-columns",
  "group-indexes",
  "group-fkeys",
  "group-triggers",
  "group-tables",
  "group-views",
  "group-procedures",
  "group-functions",
  "saved-sql-root",
  "saved-sql-folder",
]);
const pinnableTypes: Set<TreeNodeType> = new Set([
  "connection-group",
  "database",
  "schema",
  "table",
  "view",
  "redis-db",
  "mongo-db",
  "mongo-collection",
]);

function isGroupLabel(node: TreeNode): boolean {
  return groupTypes.has(node.type);
}

function displayLabel(node: TreeNode): string {
  if (node.type === "object-browser") return t(node.label, { count: node.objectCount ?? 0 });
  if (node.label === "tree.defaultDatabase") return t(node.label);
  return isGroupLabel(node) ? t(node.label) : node.label;
}

function visibleLabel(node: TreeNode): string {
  if (node.type === "table" || node.type === "view" || node.type === "mongo-collection") {
    return sidebarDisplayTableName(node.label, settingsStore.editorSettings.sidebarHiddenTablePrefixes);
  }
  return displayLabel(node);
}

function isTooltipDisabled(node: TreeNode): boolean {
  return !isTruncated.value && visibleLabel(node) === displayLabel(node);
}

async function toggle() {
  const node = props.node;
  if (node.isLoading) return;
  emit("search-toggle", node);
  const wasExpanded = !!node.isExpanded;

  if (node.type === "connection-group") {
    node.isExpanded = !node.isExpanded;
    connectionStore.toggleConnectionGroupCollapsed(node.id);
    emit("node-toggled", node, wasExpanded);
    return;
  }

  if (node.type === "saved-sql-root" || node.type === "saved-sql-folder") {
    node.isExpanded = !node.isExpanded;
    emit("node-toggled", node, wasExpanded);
    return;
  }

  if (
    node.type === "group-tables" ||
    node.type === "group-views" ||
    node.type === "group-procedures" ||
    node.type === "group-functions"
  ) {
    node.isExpanded = !node.isExpanded;
    emit("node-toggled", node, wasExpanded);
    return;
  }

  if (node.isExpanded) {
    node.isExpanded = false;
    emit("node-toggled", node, wasExpanded);
    return;
  }

  try {
    if (node.type === "connection" && node.connectionId) {
      const config = connectionStore.getConfig(node.connectionId);
      if (config?.db_type === "redis") {
        await connectionStore.loadRedisDatabases(node.connectionId);
      } else if (config?.db_type === "mongodb" || config?.db_type === "elasticsearch") {
        await connectionStore.loadMongoDatabases(node.connectionId);
      } else {
        await connectionStore.loadDatabases(node.connectionId);
      }
    } else if (node.type === "redis-db" && node.connectionId && node.database) {
      const tabTitle = `${connectionStore.getConfig(node.connectionId)?.name || "Redis"}:db${node.database}`;
      queryStore.createTab(node.connectionId, node.database, tabTitle, "redis");
    } else if (node.type === "mongo-db" && node.connectionId && node.database) {
      await connectionStore.loadMongoCollections(node.connectionId, node.database);
    } else if (node.type === "mongo-collection" && node.connectionId && node.database) {
      const tabTitle = `${node.database}.${node.label}`;
      const tab = queryStore.createTab(node.connectionId, node.database, tabTitle, "mongo");
      queryStore.updateSql(tab, node.label);
    } else if (node.type === "database" && node.connectionId && hasTreeNodeDatabaseContext(node)) {
      const config = connectionStore.getConfig(node.connectionId);
      if (config?.db_type === "sqlserver") {
        await connectionStore.loadSqlServerDatabaseObjects(node.connectionId, node.database);
      } else if (usesTreeSchemaMode(config?.db_type)) {
        await connectionStore.loadSchemas(node.connectionId, node.database);
      } else {
        await connectionStore.loadTables(node.connectionId, node.database);
      }
    } else if (node.type === "schema" && node.connectionId && hasTreeNodeDatabaseContext(node) && node.schema) {
      await connectionStore.loadTables(node.connectionId, node.database, node.schema);
    } else if (
      (node.type === "table" || node.type === "view") &&
      node.connectionId &&
      hasTreeNodeDatabaseContext(node)
    ) {
      await connectionStore.loadTableGroups(node.connectionId, node.database, node.label, node.schema, node.id);
    } else if (
      node.type === "group-columns" &&
      node.connectionId &&
      hasTreeNodeDatabaseContext(node) &&
      node.tableName
    ) {
      await connectionStore.loadColumns(node.connectionId, node.database, node.tableName, node.schema, node.id);
    } else if (
      node.type === "group-indexes" &&
      node.connectionId &&
      hasTreeNodeDatabaseContext(node) &&
      node.tableName
    ) {
      await connectionStore.loadIndexes(node.connectionId, node.database, node.tableName, node.schema, node.id);
    } else if (node.type === "group-fkeys" && node.connectionId && hasTreeNodeDatabaseContext(node) && node.tableName) {
      await connectionStore.loadForeignKeys(node.connectionId, node.database, node.tableName, node.schema, node.id);
    } else if (
      node.type === "group-triggers" &&
      node.connectionId &&
      hasTreeNodeDatabaseContext(node) &&
      node.tableName
    ) {
      await connectionStore.loadTriggers(node.connectionId, node.database, node.tableName, node.schema, node.id);
    }
    emit("node-toggled", node, wasExpanded);
  } catch (e: any) {
    if (!wasExpanded) node.isExpanded = false;
    const errMsg = e?.message || String(e);
    toast(t("connection.connectFailed", { message: translateBackendError(t, errMsg) }), 5000);
    if (errMsg.includes("driver is not installed") || errMsg.includes("is not installed")) {
      window.dispatchEvent(new Event("dbx-open-driver-store"));
    }
  }
}

function runRowClickAction() {
  const node = props.node;
  if (node.type === "object-browser") {
    void openObjectBrowser();
    return;
  }
  const action = treeNodeRowAction(node.type, canExpand.value, settingsStore.editorSettings.sidebarActivation);
  if (action === "open-data") {
    openData();
  } else if (node.type === "procedure" || node.type === "function") {
    void viewObjectSource();
  } else if (node.type === "saved-sql-file") {
    openSavedSqlFile();
  } else if (action === "toggle") {
    toggle();
  }
}

function onClick(event: MouseEvent) {
  connectionStore.selectedTreeNodeId = props.node.id;
  rowRef.value?.focus({ preventScroll: true });
  if (settingsStore.editorSettings.sidebarActivation === "double") return;
  if (event.detail > 1) return;
  runRowClickAction();
}

function isEditableShortcutTarget(target: EventTarget | null): boolean {
  if (!(target instanceof HTMLElement)) return false;
  return (
    target instanceof HTMLInputElement ||
    target instanceof HTMLTextAreaElement ||
    target.isContentEditable ||
    !!target.closest("[contenteditable='true']")
  );
}

function onKeydown(event: KeyboardEvent) {
  if (!isSelected.value || isEditableShortcutTarget(event.target)) return;
  const action = sidebarSelectionCopyAction(event);
  if (action !== "copy-name") return;
  event.preventDefault();
  event.stopPropagation();
  copyName();
}

function onDoubleClick() {
  const action = treeNodeRowDoubleClickAction(
    props.node.type,
    canOpenObjectBrowser.value,
    settingsStore.editorSettings.sidebarActivation,
    canExpand.value,
  );
  if (action === "open-object-browser") {
    void openObjectBrowser();
  } else if (action === "open-object-browser-and-expand") {
    void openObjectBrowser();
    if (!props.node.isExpanded) void toggle();
  } else if (action === "open-data") {
    openData();
  } else if (action === "open-source") {
    void viewObjectSource();
  } else if (action === "open-saved-sql") {
    openSavedSqlFile();
  } else if (action === "toggle") {
    toggle();
  }
}

async function openObjectBrowser() {
  const node = props.node;
  if (!node.connectionId) return;
  try {
    await connectionStore.ensureConnected(node.connectionId);
    connectionStore.activeConnectionId = node.connectionId;

    if (hasTreeNodeDatabaseContext(node)) {
      queryStore.openObjectBrowser(node.connectionId, node.database, node.schema);
      return;
    }

    const connection = connectionStore.getConfig(node.connectionId);
    if (!connection) return;
    const options = await getDatabaseOptions(node.connectionId);
    const database = resolveDefaultDatabase(connection, options);
    if (database) {
      queryStore.openObjectBrowser(node.connectionId, database);
    } else {
      await toggle();
    }
  } catch (e: any) {
    toast(t("connection.connectFailed", { message: translateBackendError(t, e?.message || String(e)) }), 5000);
    if (
      e?.message?.includes("driver is not installed") ||
      (e?.message?.includes("JRE") && e?.message?.includes("not installed"))
    ) {
      window.dispatchEvent(new Event("dbx-open-driver-store"));
    }
  }
}

function openSavedSqlFile() {
  const id = props.node.savedSqlId;
  if (!id) return;
  const file = savedSqlStore.getFile(id);
  if (!file) return;
  queryStore.openSavedSql(file);
  connectionStore.activeConnectionId = file.connectionId;
}

async function openData() {
  const node = props.node;
  if (!(node.type === "table" || node.type === "view") || !hasNodeDatabaseContext(node)) return;
  const config = connectionStore.getConfig(node.connectionId);
  const traceId = uuid().slice(0, 8);
  const startedAt = performance.now();
  const elapsed = () => `${Math.round(performance.now() - startedAt)}ms`;
  console.info("[DBX][openData:start]", {
    traceId,
    type: node.type,
    connectionId: node.connectionId,
    database: node.database,
    schema: node.schema,
    table: node.label,
    dbType: config?.db_type,
  });
  const tabId = queryStore.createTab(node.connectionId, node.database, node.label, "data", node.schema);
  console.info("[DBX][openData:tab-created]", { traceId, tabId, elapsed: elapsed() });
  queryStore.setExecuting(tabId, true);

  try {
    console.info("[DBX][openData:ensure-connected:start]", { traceId, elapsed: elapsed() });
    await connectionStore.ensureConnected(node.connectionId);
    console.info("[DBX][openData:ensure-connected:done]", { traceId, elapsed: elapsed() });
    if (!config) throw new Error("Connection config not found");

    const querySchema = node.schema || node.database;
    console.info("[DBX][openData:get-columns:start]", {
      traceId,
      database: node.database,
      schema: querySchema,
      table: node.label,
      elapsed: elapsed(),
    });
    const columns = await api.getColumns(node.connectionId, node.database, querySchema, node.label);
    console.info("[DBX][openData:get-columns:done]", {
      traceId,
      columnCount: columns.length,
      primaryKeys: columns.filter((column) => column.is_primary_key).map((column) => column.name),
      elapsed: elapsed(),
    });
    const pks = editablePrimaryKeys(config.db_type, columns);
    const limit = settingsStore.editorSettings.pageSize;
    const sql = await buildTableSelectSql({
      databaseType: config.db_type,
      schema: node.schema,
      tableName: node.label,
      columns: columns.map((column) => column.name),
      primaryKeys: pks,
      limit,
      includeRowId: usesSyntheticRowIdKey(config.db_type, pks),
    });
    console.info("[DBX][openData:sql-built]", {
      traceId,
      primaryKeys: pks,
      includeRowId: usesSyntheticRowIdKey(config.db_type, pks),
      sql,
      elapsed: elapsed(),
    });
    queryStore.updateSql(tabId, sql);
    queryStore.setTableMeta(tabId, {
      schema: node.schema,
      tableName: node.label,
      columns,
      primaryKeys: pks,
    });

    console.info("[DBX][openData:execute:start]", { traceId, tabId, elapsed: elapsed() });
    await queryStore.executeTabSql(tabId, sql);
    console.info("[DBX][openData:execute:done]", { traceId, tabId, elapsed: elapsed() });
  } catch (e: any) {
    console.error("[DBX][openData:error]", { traceId, elapsed: elapsed(), error: e });
    queryStore.setErrorResult(tabId, e);
  }
}

async function newQuery() {
  const node = props.node;
  if (!node.connectionId) return;
  try {
    await connectionStore.ensureConnected(node.connectionId);
    connectionStore.activeConnectionId = node.connectionId;
    if (hasTreeNodeDatabaseContext(node)) {
      queryStore.createTab(node.connectionId, node.database, undefined, "query", node.schema);
      return;
    }
    const connection = connectionStore.getConfig(node.connectionId);
    if (!connection) return;
    const options = await getDatabaseOptions(node.connectionId);
    queryStore.createTab(node.connectionId, resolveDefaultDatabase(connection, options), undefined, "query");
  } catch (e: any) {
    toast(t("connection.connectFailed", { message: translateBackendError(t, e?.message || String(e)) }), 5000);
    if (
      e?.message?.includes("driver is not installed") ||
      (e?.message?.includes("JRE") && e?.message?.includes("not installed"))
    ) {
      window.dispatchEvent(new Event("dbx-open-driver-store"));
    }
  }
}

async function setNodeAsDefaultDatabase() {
  const node = props.node;
  if (!node.connectionId || !node.database) return;
  try {
    await connectionStore.setDefaultDatabase(node.connectionId, node.database);
  } catch (e: any) {
    toast(t("connection.saveFailed", { message: e?.message || String(e) }), 5000);
  }
}

async function clearNodeDefaultDatabase() {
  const node = props.node;
  if (!node.connectionId) return;
  try {
    await connectionStore.clearDefaultDatabase(node.connectionId);
  } catch (e: any) {
    toast(t("connection.saveFailed", { message: e?.message || String(e) }), 5000);
  }
}

async function refresh() {
  try {
    await connectionStore.refreshTreeNode(props.node);
  } catch (e: any) {
    toast(t("connection.connectFailed", { message: translateBackendError(t, e?.message || String(e)) }), 5000);
    if (
      e?.message?.includes("driver is not installed") ||
      (e?.message?.includes("JRE") && e?.message?.includes("not installed"))
    ) {
      window.dispatchEvent(new Event("dbx-open-driver-store"));
    }
  }
}

const showDeleteConfirm = ref(false);

function deleteConnection() {
  showDeleteConfirm.value = true;
}

async function confirmDelete() {
  const node = props.node;
  if (node.connectionId) {
    try {
      await connectionStore.disconnect(node.connectionId);
      await connectionStore.removeConnection(node.connectionId);
      toast(t("connection.deleted"), 2000);
    } catch (e: any) {
      toast(t("connection.saveFailed", { message: e?.message || String(e) }), 5000);
    }
  }
}

async function copyName() {
  try {
    await copyToClipboard(props.node.label);
    toast(t("connection.copied"), 2000);
  } catch (e: any) {
    toast(t("grid.copyFailed", { message: e?.message || String(e) }), 5000);
  }
}

async function duplicateConnection() {
  const connId = props.node.connectionId;
  if (!connId) return;
  const config = connectionStore.getConfig(connId);
  if (!config) return;
  const newConfig = { ...config, id: uuid(), name: `${config.name} (Copy)` };
  await connectionStore.addConnection(newConfig);
  toast(t("connection.duplicated"), 2000);
}

// --- Table Management Operations ---
const showDropTableConfirm = ref(false);
const showEmptyTableConfirm = ref(false);
const showTruncateTableConfirm = ref(false);
const showRenameObjectDialog = ref(false);
const renameObjectName = ref("");
const renameObjectError = ref("");
const renameObjectPreviewSql = ref("");
const dropTablePreviewSql = ref("");
const emptyTablePreviewSql = ref("");
const truncateTablePreviewSql = ref("");
const dropObjectPreviewSql = ref("");
const dropDatabasePreviewSql = ref("");
const dropSchemaPreviewSql = ref("");
const showDuplicateDialog = ref(false);
const duplicateTableName = ref("");

const showCreateDatabaseDialog = ref(false);
const createDatabaseName = ref("");
const createDatabaseCharset = ref("utf8mb4");
const createDatabaseCollation = ref("utf8mb4_unicode_ci");
const showDropDatabaseConfirm = ref(false);
const showFlushRedisDbConfirm = ref(false);
const showCreateSchemaDialog = ref(false);
const createSchemaName = ref("");
const showDropSchemaConfirm = ref(false);

// --- Procedure / Function Management ---
const showDropObjectConfirm = ref(false);

function dropObjectSqlOptions(): DropObjectSqlOptions | null {
  const node = props.node;
  if (node.type !== "procedure" && node.type !== "function") return null;
  return {
    databaseType: currentDatabaseType(),
    objectType: node.type === "procedure" ? "PROCEDURE" : "FUNCTION",
    schema: node.schema,
    name: node.label,
  };
}

async function refreshDropObjectPreviewSql() {
  const options = dropObjectSqlOptions();
  dropObjectPreviewSql.value = "";
  dropObjectPreviewSql.value = options ? await buildDropObjectSql(options).catch(() => "") : "";
}

function viewObjectSource() {
  const node = props.node;
  if (!node.connectionId || !node.database) return;
  const objectType = objectSourceKindForTreeNode(node.type);
  if (!objectType) return;
  const schema = node.schema || node.database;
  connectionStore
    .ensureConnected(node.connectionId)
    .then(() => {
      connectionStore.activeConnectionId = node.connectionId!;
      return api.getObjectSource(node.connectionId!, node.database!, schema, node.label, objectType as any);
    })
    .then(async (result) => {
      const tabId = queryStore.createTab(node.connectionId!, node.database!, `Source - ${node.label}`);
      queryStore.updateSql(tabId, result.source);
      queryStore.setObjectSource(tabId, {
        schema,
        name: node.label,
        objectType,
      });
    })
    .catch((e: any) => {
      toast(e?.message || String(e), 5000);
    });
}

function viewObjectDdl() {
  const node = props.node;
  if (node.type !== "view" || !node.connectionId || !node.database) return;
  const schema = node.schema || node.database;
  connectionStore
    .ensureConnected(node.connectionId)
    .then(() => {
      connectionStore.activeConnectionId = node.connectionId!;
      return api.getObjectSource(node.connectionId!, node.database!, schema, node.label, "VIEW");
    })
    .then(async (result) => {
      const connection = connectionStore.getConfig(node.connectionId!);
      const ddl = await buildViewDdl({
        databaseType: connection?.db_type,
        schema,
        name: node.label,
        source: result.source,
      });
      const tabId = queryStore.createTab(node.connectionId!, node.database!, `DDL - ${node.label}`);
      queryStore.updateSql(tabId, ddl);
    })
    .catch((e: any) => {
      toast(e?.message || String(e), 5000);
    });
}

function requestDropObject() {
  void refreshDropObjectPreviewSql();
  showDropObjectConfirm.value = true;
}

function nodeRenameObjectType(): RenameableObjectType | null {
  if (props.node.type === "table") return "TABLE";
  if (props.node.type === "view") return "VIEW";
  if (props.node.type === "procedure") return "PROCEDURE";
  if (props.node.type === "function") return "FUNCTION";
  return null;
}

const canRenameObject = computed(() => {
  const objectType = nodeRenameObjectType();
  return (
    !!objectType &&
    (supportsObjectRename(currentDatabaseType(), objectType) ||
      supportsSourceBackedRoutineRename(currentDatabaseType(), objectType as any))
  );
});

function openRenameObjectDialog() {
  renameObjectName.value = props.node.label;
  renameObjectError.value = "";
  renameObjectPreviewSql.value = "";
  showRenameObjectDialog.value = true;
}

let renameObjectPreviewRequestId = 0;

async function refreshRenameObjectPreviewSql() {
  const requestId = ++renameObjectPreviewRequestId;
  const objectType = nodeRenameObjectType();
  const newName = renameObjectName.value.trim();
  if (!showRenameObjectDialog.value || !objectType || !newName || newName === props.node.label) {
    renameObjectPreviewSql.value = "";
    return;
  }
  if (supportsSourceBackedRoutineRename(currentDatabaseType(), objectType as any)) {
    renameObjectPreviewSql.value = `-- Recreate ${objectType} from source, then drop the original object.`;
    return;
  }
  try {
    const sql = await buildRenameObjectSql({
      databaseType: currentDatabaseType(),
      objectType,
      schema: props.node.schema,
      oldName: props.node.label,
      newName,
    });
    if (requestId === renameObjectPreviewRequestId) renameObjectPreviewSql.value = sql;
  } catch {
    if (requestId === renameObjectPreviewRequestId) renameObjectPreviewSql.value = "";
  }
}

watch(
  [
    showRenameObjectDialog,
    renameObjectName,
    () => props.node.label,
    () => props.node.schema,
    () => props.node.type,
    () => currentDatabaseType(),
  ],
  () => {
    void refreshRenameObjectPreviewSql();
  },
);

async function confirmRenameObject() {
  const node = props.node;
  const objectType = nodeRenameObjectType();
  const newName = renameObjectName.value.trim();
  if (!objectType || !newName || newName === node.label || !node.connectionId || !node.database) return;
  renameObjectError.value = "";
  try {
    const dbType = currentDatabaseType();
    await connectionStore.ensureConnected(node.connectionId);
    if (supportsSourceBackedRoutineRename(dbType, objectType as any)) {
      const schema = node.schema || node.database;
      const source = await api.getObjectSource(node.connectionId, node.database, schema, node.label, objectType as any);
      const statements = await buildRoutineRenameObjectSourceStatements({
        databaseType: dbType!,
        objectType: objectType as any,
        schema,
        name: node.label,
        newName,
        source: source.source,
      });
      for (const sql of statements) {
        await api.executeQuery(node.connectionId, node.database, sql, schema);
      }
    } else {
      const sql = await buildRenameObjectSql({
        databaseType: dbType,
        objectType,
        schema: node.schema,
        oldName: node.label,
        newName,
      });
      await api.executeQuery(node.connectionId, node.database, sql, node.schema);
    }
    toast(t("contextMenu.renameObjectSuccess", { oldName: node.label, newName }), 3000);
    showRenameObjectDialog.value = false;
    await refreshTableList(node);
  } catch (e: any) {
    renameObjectError.value = e?.message || String(e);
  }
}

async function confirmDropObject() {
  const node = props.node;
  if (!node.connectionId || !node.database) return;
  const options = dropObjectSqlOptions();
  if (!options) return;
  try {
    await connectionStore.ensureConnected(node.connectionId);
    const sql = dropObjectPreviewSql.value || (await buildDropObjectSql(options));
    await api.executeQuery(node.connectionId, node.database, sql, node.schema);
    const msgKey = node.type === "procedure" ? "contextMenu.dropProcedureSuccess" : "contextMenu.dropFunctionSuccess";
    toast(t(msgKey, { name: node.label }), 3000);
    await refreshTableList(node);
  } catch (e: any) {
    toast(t("contextMenu.tableOperationFailed", { message: e?.message || String(e) }), 5000);
  }
}

const isTableNotView = computed(() => props.node.type === "table");

const supportsTruncate = computed(() => {
  return supportsTableTruncate(currentDatabaseType());
});

const canCreateTable = computed(() => {
  const config = props.node.connectionId ? connectionStore.getConfig(props.node.connectionId) : undefined;
  return (
    (props.node.type === "database" || props.node.type === "schema") &&
    !!props.node.database &&
    supportsTableStructureEditing(config?.db_type)
  );
});

const canCreateDatabase = computed(() => {
  const config = props.node.connectionId ? connectionStore.getConfig(props.node.connectionId) : undefined;
  return (
    props.node.type === "connection" && (supportsDatabaseCreation(config?.db_type) || config?.db_type === "duckdb")
  );
});

const isDuckDbConnection = computed(() => {
  const config = props.node.connectionId ? connectionStore.getConfig(props.node.connectionId) : undefined;
  return props.node.type === "connection" && config?.db_type === "duckdb";
});

const canSetCreateDatabaseCharset = computed(() => {
  const config = props.node.connectionId ? connectionStore.getConfig(props.node.connectionId) : undefined;
  return supportsCreateDatabaseCharset(config?.db_type, config?.driver_profile);
});

const canDropDatabase = computed(() => {
  const config = props.node.connectionId ? connectionStore.getConfig(props.node.connectionId) : undefined;
  return props.node.type === "database" && supportsDatabaseCreation(config?.db_type);
});

const canCreateSchema = computed(() => {
  const config = props.node.connectionId ? connectionStore.getConfig(props.node.connectionId) : undefined;
  return props.node.type === "database" && usesTreeSchemaMode(config?.db_type);
});

const canDropSchema = computed(() => {
  const config = props.node.connectionId ? connectionStore.getConfig(props.node.connectionId) : undefined;
  return props.node.type === "schema" && usesTreeSchemaMode(config?.db_type);
});

function tableAdminSqlOptions(): TableAdminSqlOptions {
  return {
    databaseType: currentDatabaseType(),
    schema: props.node.schema,
    tableName: props.node.label,
  };
}

async function refreshDropTablePreviewSql() {
  dropTablePreviewSql.value = "";
  dropTablePreviewSql.value = await buildDropTableSql(tableAdminSqlOptions()).catch(() => "");
}

async function refreshEmptyTablePreviewSql() {
  emptyTablePreviewSql.value = "";
  emptyTablePreviewSql.value = await buildEmptyTableSql(tableAdminSqlOptions()).catch(() => "");
}

async function refreshTruncateTablePreviewSql() {
  truncateTablePreviewSql.value = "";
  truncateTablePreviewSql.value = await buildTruncateTableSql(tableAdminSqlOptions()).catch(() => "");
}

function dropTable() {
  void refreshDropTablePreviewSql();
  showDropTableConfirm.value = true;
}

async function refreshTableList(node: TreeNode) {
  if (!node.connectionId || !node.database) return;
  await connectionStore.refreshObjectListTreeNode(node.connectionId, node.database, node.schema);
}

async function confirmDropTable() {
  const node = props.node;
  if (!node.connectionId || !node.database) return;
  try {
    await connectionStore.ensureConnected(node.connectionId);
    const sql = dropTablePreviewSql.value || (await buildDropTableSql(tableAdminSqlOptions()));
    await api.executeQuery(node.connectionId, node.database, sql, node.schema);
    toast(t("contextMenu.dropTableSuccess", { name: node.label }), 3000);
    connectionStore.removeTreeNode(node.id);
  } catch (e: any) {
    toast(t("contextMenu.tableOperationFailed", { message: e?.message || String(e) }), 5000);
  }
}

function emptyTable() {
  void refreshEmptyTablePreviewSql();
  showEmptyTableConfirm.value = true;
}

async function confirmEmptyTable() {
  const node = props.node;
  if (!node.connectionId || !node.database) return;
  try {
    await connectionStore.ensureConnected(node.connectionId);
    const sql = emptyTablePreviewSql.value || (await buildEmptyTableSql(tableAdminSqlOptions()));
    await api.executeQuery(node.connectionId, node.database, sql, node.schema);
    toast(t("contextMenu.emptyTableSuccess", { name: node.label }), 3000);
  } catch (e: any) {
    toast(t("contextMenu.tableOperationFailed", { message: e?.message || String(e) }), 5000);
  }
}

function truncateTable() {
  void refreshTruncateTablePreviewSql();
  showTruncateTableConfirm.value = true;
}

async function confirmTruncateTable() {
  const node = props.node;
  if (!node.connectionId || !node.database) return;
  try {
    await connectionStore.ensureConnected(node.connectionId);
    const sql = truncateTablePreviewSql.value || (await buildTruncateTableSql(tableAdminSqlOptions()));
    await api.executeQuery(node.connectionId, node.database, sql, node.schema);
    toast(t("contextMenu.truncateTableSuccess", { name: node.label }), 3000);
  } catch (e: any) {
    toast(t("contextMenu.tableOperationFailed", { message: e?.message || String(e) }), 5000);
  }
}

async function refreshDropDatabasePreviewSql() {
  dropDatabasePreviewSql.value = "";
  dropDatabasePreviewSql.value = await buildDropDatabaseSql({
    databaseType: currentDatabaseType(),
    name: props.node.label,
  }).catch(() => "");
}

async function refreshDropSchemaPreviewSql() {
  dropSchemaPreviewSql.value = "";
  dropSchemaPreviewSql.value = await buildDropSchemaSql({
    databaseType: currentDatabaseType(),
    name: props.node.label,
  }).catch(() => "");
}

async function openCreateDatabase() {
  if (isDuckDbConnection.value) {
    await createDuckDbAttachedDatabaseFile();
    return;
  }
  openCreateDatabaseDialog();
}

function openCreateDatabaseDialog() {
  createDatabaseName.value = "";
  createDatabaseCharset.value = "utf8mb4";
  createDatabaseCollation.value = "utf8mb4_unicode_ci";
  showCreateDatabaseDialog.value = true;
}

function ensureDuckDbFileExtension(path: string): string {
  return /\.(duckdb|db)$/i.test(path) ? path : `${path}.duckdb`;
}

async function createDuckDbAttachedDatabaseFile() {
  const node = props.node;
  if (!node.connectionId) return;
  if (!isTauriRuntime()) {
    toast(t("contextMenu.createDuckDbFileDesktopOnly"), 4000);
    return;
  }

  try {
    const { save } = await import("@tauri-apps/plugin-dialog");
    const selectedPath = await save({
      defaultPath: "database.duckdb",
      filters: [{ name: "DuckDB", extensions: ["duckdb", "db"] }],
    });
    if (!selectedPath) return;

    const path = ensureDuckDbFileExtension(selectedPath);
    await connectionStore.ensureConnected(node.connectionId);
    const existingDatabases = await api.listDatabases(node.connectionId);
    const name = uniqueDuckDbAttachedDatabaseName(
      duckDbAttachedDatabaseNameFromPath(path),
      existingDatabases.map((database) => database.name),
    );
    await api.executeQuery(node.connectionId, "", await buildDuckDbAttachDatabaseSql(path, name));

    const config = connectionStore.getConfig(node.connectionId);
    if (config) {
      await connectionStore.updateConnection({
        ...config,
        attached_databases: [...(config.attached_databases ?? []), { name, path }],
      });
    }
    await connectionStore.loadDatabases(node.connectionId, { force: true });
    connectionStore.selectedTreeNodeId = `${node.connectionId}:${name}`;
    toast(t("contextMenu.createDuckDbFileSuccess", { name }), 3000);
  } catch (e: any) {
    toast(t("contextMenu.tableOperationFailed", { message: e?.message || String(e) }), 5000);
  }
}

async function confirmCreateDatabase() {
  const node = props.node;
  const name = createDatabaseName.value.trim();
  if (!name || !node.connectionId) return;
  showCreateDatabaseDialog.value = false;
  try {
    await connectionStore.ensureConnected(node.connectionId);
    const config = connectionStore.getConfig(node.connectionId);
    const sql = await buildCreateDatabaseSql({
      databaseType: config?.db_type,
      driverProfile: config?.driver_profile,
      name,
      charset: createDatabaseCharset.value,
      collation: createDatabaseCollation.value,
    });
    await api.executeQuery(node.connectionId, "", sql);
    toast(t("contextMenu.createDatabaseSuccess", { name }), 3000);
    await connectionStore.loadDatabases(node.connectionId, { force: true });
  } catch (e: any) {
    toast(t("contextMenu.tableOperationFailed", { message: e?.message || String(e) }), 5000);
  }
}

function dropDatabase() {
  void refreshDropDatabasePreviewSql();
  showDropDatabaseConfirm.value = true;
}

function flushRedisDb() {
  showFlushRedisDbConfirm.value = true;
}

async function confirmFlushRedisDb() {
  const node = props.node;
  if (node.type !== "redis-db" || !node.connectionId || !node.database) return;
  try {
    await connectionStore.ensureConnected(node.connectionId);
    await api.redisFlushDb(node.connectionId, Number(node.database));
    connectionStore.updateRedisDbKeyStats(node.connectionId, Number(node.database), { loaded: 0, total: 0 });
    window.dispatchEvent(
      new CustomEvent("dbx-redis-db-flushed", {
        detail: { connectionId: node.connectionId, db: Number(node.database) },
      }),
    );
    toast(t("redis.flushDbSuccess", { db: node.database }), 3000);
  } catch (e: any) {
    toast(t("contextMenu.tableOperationFailed", { message: e?.message || String(e) }), 5000);
  }
}

async function confirmDropDatabase() {
  const node = props.node;
  if (!node.connectionId) return;
  try {
    await connectionStore.ensureConnected(node.connectionId);
    const sql =
      dropDatabasePreviewSql.value ||
      (await buildDropDatabaseSql({
        databaseType: currentDatabaseType(),
        name: node.label,
      }));
    await api.executeQuery(node.connectionId, "", sql);
    toast(t("contextMenu.dropDatabaseSuccess", { name: node.label }), 3000);
    await connectionStore.loadDatabases(node.connectionId, { force: true });
  } catch (e: any) {
    toast(t("contextMenu.tableOperationFailed", { message: e?.message || String(e) }), 5000);
  }
}

function openCreateSchemaDialog() {
  createSchemaName.value = "";
  showCreateSchemaDialog.value = true;
}

async function confirmCreateSchema() {
  const node = props.node;
  const name = createSchemaName.value.trim();
  if (!name || !node.connectionId || !node.database) return;
  showCreateSchemaDialog.value = false;
  try {
    await connectionStore.ensureConnected(node.connectionId);
    const sql = await buildCreateSchemaSql({
      databaseType: currentDatabaseType(),
      name,
    });
    await api.executeQuery(node.connectionId, node.database, sql);
    toast(t("contextMenu.createSchemaSuccess", { name }), 3000);
    const config = connectionStore.getConfig(node.connectionId);
    if (config?.db_type === "sqlserver") {
      await connectionStore.loadSqlServerDatabaseObjects(node.connectionId, node.database, { force: true });
    } else {
      await connectionStore.loadSchemas(node.connectionId, node.database, { force: true });
    }
  } catch (e: any) {
    toast(t("contextMenu.tableOperationFailed", { message: e?.message || String(e) }), 5000);
  }
}

function dropSchema() {
  void refreshDropSchemaPreviewSql();
  showDropSchemaConfirm.value = true;
}

async function confirmDropSchema() {
  const node = props.node;
  if (!node.connectionId || !node.database) return;
  try {
    await connectionStore.ensureConnected(node.connectionId);
    const sql =
      dropSchemaPreviewSql.value ||
      (await buildDropSchemaSql({
        databaseType: currentDatabaseType(),
        name: node.label,
      }));
    await api.executeQuery(node.connectionId, node.database, sql);
    toast(t("contextMenu.dropSchemaSuccess", { name: node.label }), 3000);
    const config = connectionStore.getConfig(node.connectionId);
    if (config?.db_type === "sqlserver") {
      await connectionStore.loadSqlServerDatabaseObjects(node.connectionId, node.database, { force: true });
    } else {
      await connectionStore.loadSchemas(node.connectionId, node.database, { force: true });
    }
  } catch (e: any) {
    toast(t("contextMenu.tableOperationFailed", { message: e?.message || String(e) }), 5000);
  }
}

function duplicateStructure() {
  duplicateTableName.value = `${props.node.label}_copy`;
  showDuplicateDialog.value = true;
}

async function confirmDuplicateStructure() {
  const node = props.node;
  const newName = duplicateTableName.value.trim();
  if (!newName || !node.connectionId || !node.database) return;
  showDuplicateDialog.value = false;
  try {
    await connectionStore.ensureConnected(node.connectionId);
    const sql = await buildDuplicateTableStructureSql({
      databaseType: currentDatabaseType(),
      schema: node.schema,
      sourceName: node.label,
      targetName: newName,
    });
    await api.executeQuery(node.connectionId, node.database, sql, node.schema);
    toast(t("contextMenu.duplicateStructureSuccess", { name: newName }), 3000);
    await refreshTableList(node);
  } catch (e: any) {
    toast(t("contextMenu.tableOperationFailed", { message: e?.message || String(e) }), 5000);
  }
}

function createTable() {
  const node = props.node;
  if (!node.connectionId || !node.database) return;
  queryStore.openTableStructure(node.connectionId, node.database, node.schema, "");
}

async function saveFileContent(content: string, defaultFileName: string, filterName: string, filterExt: string) {
  if (isTauriRuntime()) {
    const { save } = await import("@tauri-apps/plugin-dialog");
    const { writeTextFile } = await import("@tauri-apps/plugin-fs");
    const path = await save({
      defaultPath: defaultFileName,
      filters: [{ name: filterName, extensions: [filterExt] }],
    });
    if (path) await writeTextFile(path, content);
  } else {
    const blob = new Blob([content], { type: "text/plain" });
    const url = URL.createObjectURL(blob);
    const a = document.createElement("a");
    a.href = url;
    a.download = defaultFileName;
    a.click();
    URL.revokeObjectURL(url);
  }
}

async function exportStructure() {
  const node = props.node;
  if (!node.connectionId || !node.database) return;
  try {
    await connectionStore.ensureConnected(node.connectionId);
    const ddl = await api.getTableDdl(node.connectionId, node.database, node.schema || node.database, node.label);
    await saveFileContent(ddl + "\n", `${node.label}.sql`, "SQL", "sql");
  } catch (e: any) {
    console.error("Export structure failed:", e);
  }
}

async function exportData(format: "csv" | "json" | "sql") {
  const node = props.node;
  if (!node.connectionId || !node.database) return;
  const connectionId = node.connectionId;
  const database = node.database;
  const config = connectionStore.getConfig(node.connectionId);
  if (!config) return;

  try {
    await connectionStore.ensureConnected(connectionId);
    const queryColumns =
      config.db_type === "neo4j"
        ? (await api.getColumns(connectionId, database, node.schema || database, node.label)).map(
            (column) => column.name,
          )
        : undefined;
    const result = await fetchTableDataForExport({
      databaseType: config.db_type,
      schema: node.schema,
      tableName: node.label,
      columns: queryColumns,
      executePage: (sql) => api.executeQuery(connectionId, database, sql),
    });

    if (format === "csv") {
      let outputPath = `${node.label}.csv`;
      if (isTauriRuntime()) {
        const { save } = await import("@tauri-apps/plugin-dialog");
        const path = await save({
          defaultPath: outputPath,
          filters: [{ name: "CSV", extensions: ["csv"] }],
        });
        if (!path) return;
        outputPath = path as string;
      }
      await api.exportQueryResultCsv(outputPath, result.columns, result.rows);
      toast(t("grid.exported"));
      return;
    }

    if (format === "json") {
      let outputPath = `${node.label}.json`;
      if (isTauriRuntime()) {
        const { save } = await import("@tauri-apps/plugin-dialog");
        const path = await save({
          defaultPath: outputPath,
          filters: [{ name: "JSON", extensions: ["json"] }],
        });
        if (!path) return;
        outputPath = path as string;
      }
      await api.exportQueryResultJson(outputPath, result.columns, result.rows);
      toast(t("grid.exported"));
      return;
    }

    const content = await formatSqlInsert({
      databaseType: config.db_type,
      schema: node.schema,
      tableName: node.label,
      columns: result.columns,
      rows: result.rows,
    });
    await saveFileContent(content, `${node.label}.sql`, "SQL", "sql");
    toast(t("grid.exported"));
  } catch (e: any) {
    toast(t("grid.exportFailed", { message: e?.message || String(e) }), 5000);
  }
}

async function exportDataXlsx() {
  const node = props.node;
  if (!node.connectionId || !node.database) return;
  const connectionId = node.connectionId;
  const database = node.database;
  const config = connectionStore.getConfig(node.connectionId);
  if (!config) return;

  try {
    await connectionStore.ensureConnected(connectionId);
    const queryColumns =
      config.db_type === "neo4j"
        ? (await api.getColumns(connectionId, database, node.schema || database, node.label)).map(
            (column) => column.name,
          )
        : undefined;
    const result = await fetchTableDataForExport({
      databaseType: config.db_type,
      schema: node.schema,
      tableName: node.label,
      columns: queryColumns,
      executePage: (sql) => api.executeQuery(connectionId, database, sql),
    });

    let outputPath = `${node.label}.xlsx`;
    if (isTauriRuntime()) {
      const { save } = await import("@tauri-apps/plugin-dialog");
      const path = await save({
        defaultPath: outputPath,
        filters: [{ name: "Excel", extensions: ["xlsx"] }],
      });
      if (!path) return;
      outputPath = path as string;
    }
    await api.exportQueryResultXlsx(outputPath, node.label, result.columns, result.rows);
    toast(t("grid.exported"));
  } catch (e: any) {
    toast(t("grid.exportFailed", { message: e?.message || String(e) }), 5000);
  }
}

function editConnection() {
  if (props.node.connectionId) {
    connectionStore.startEditing(props.node.connectionId);
  }
}

function disconnectConnection() {
  if (props.node.connectionId) {
    connectionStore.disconnect(props.node.connectionId);
    props.node.isExpanded = false;
    props.node.children = [];
    toast(t("connection.disconnected"), 2000);
  }
}

function openTransfer() {
  if (props.node.connectionId) {
    connectionStore.transferSource = {
      connectionId: props.node.connectionId,
      database: props.node.database ?? "",
    };
  }
}

function openSchemaDiff() {
  if (props.node.connectionId) {
    connectionStore.schemaDiffSource = {
      connectionId: props.node.connectionId,
      database: props.node.database ?? "",
      schema: props.node.schema,
    };
  }
}

function openDataCompare() {
  if (props.node.connectionId) {
    connectionStore.dataCompareSource = {
      connectionId: props.node.connectionId,
      database: props.node.database ?? "",
      schema: props.node.schema,
      tableName: props.node.type === "table" ? props.node.label : undefined,
    };
  }
}

function openSqlFileExecution() {
  if (props.node.connectionId) {
    connectionStore.sqlFileSource = {
      connectionId: props.node.connectionId,
      database: props.node.database ?? "",
    };
  }
}

function openDiagram() {
  const node = props.node;
  if (!node.connectionId || !node.database) return;
  connectionStore.diagramSource = {
    connectionId: node.connectionId,
    database: node.database,
    schema: node.schema,
    tableName: node.type === "table" ? node.label : undefined,
  };
}

function openDatabaseSearch() {
  const node = props.node;
  if (!node.connectionId || !node.database) return;
  connectionStore.databaseSearchSource = {
    connectionId: node.connectionId,
    database: node.database,
    schema: node.type === "schema" ? node.schema : undefined,
  };
}

function openDatabaseExport() {
  const node = props.node;
  if (!node.connectionId || !node.database) return;
  connectionStore.databaseExportSource = {
    connectionId: node.connectionId,
    database: node.database,
    schema: node.type === "schema" || node.type === "table" || node.type === "view" ? node.schema : undefined,
    tableName: node.type === "table" || node.type === "view" ? node.label : undefined,
  };
}

function openTableImport() {
  const node = props.node;
  if (node.type !== "table" || !node.connectionId || !node.database) return;
  connectionStore.tableImportSource = {
    connectionId: node.connectionId,
    database: node.database,
    schema: node.schema,
    tableName: node.label,
  };
}

function openStructureEditor() {
  const node = props.node;
  if (node.type !== "table" || !node.connectionId || !node.database) return;
  queryStore.openTableStructure(node.connectionId, node.database, node.schema, node.label);
}

function openFieldLineage() {
  const node = props.node;
  const column = node.type === "column" && node.meta && "name" in node.meta ? node.meta.name : node.label;
  if (node.type !== "column" || !node.connectionId || !node.database || !node.tableName || !column) return;
  connectionStore.fieldLineageSource = {
    connectionId: node.connectionId,
    database: node.database,
    schema: node.schema,
    tableName: node.tableName,
    columnName: column,
  };
}

const canExpand = computed(() =>
  canTreeNodeShowExpander({
    type: props.node.type,
    childCount: props.node.children?.length ?? 0,
  }),
);
const nodeConfig = computed(() =>
  props.node.connectionId ? connectionStore.getConfig(props.node.connectionId) : undefined,
);
const canPin = computed(() => pinnableTypes.has(props.node.type));
const canOpenSqlFileExecution = computed(() => {
  return supportsSqlFileExecution(nodeConfig.value?.db_type);
});
const canOpenDiagram = computed(() => {
  return !!props.node.database && supportsSchemaDiagram(nodeConfig.value?.db_type);
});
const canOpenDatabaseSearch = computed(() => {
  return !!props.node.database && supportsDatabaseSearch(nodeConfig.value?.db_type);
});
const canOpenObjectBrowser = computed(() => {
  return supportsObjectBrowserTreeNode(nodeConfig.value?.db_type, props.node.type);
});
const canOpenTableImport = computed(() => {
  return props.node.type === "table" && !!props.node.database && supportsTableImport(nodeConfig.value?.db_type);
});
const canOpenStructureEditor = computed(() => {
  return (
    props.node.type === "table" && !!props.node.database && supportsTableStructureEditing(nodeConfig.value?.db_type)
  );
});
const canOpenFieldLineage = computed(() => {
  return (
    props.node.type === "column" &&
    !!props.node.database &&
    !!props.node.tableName &&
    supportsFieldLineage(nodeConfig.value?.db_type)
  );
});
const isPinned = computed(() => props.node.pinned || connectionStore.isTreeNodePinned(props.node.id));
const isNodeDefaultDatabase = computed(
  () =>
    (props.node.type === "database" || props.node.type === "redis-db" || props.node.type === "mongo-db") &&
    !!props.node.connectionId &&
    !!props.node.database &&
    connectionStore.isDefaultDatabase(props.node.connectionId, props.node.database),
);
const hasTypeMenu = computed(() => {
  const t = props.node.type;
  return (
    t === "connection" ||
    t === "database" ||
    t === "schema" ||
    t === "table" ||
    t === "view" ||
    t === "column" ||
    t === "procedure" ||
    t === "function" ||
    t === "saved-sql-root" ||
    t === "saved-sql-folder" ||
    t === "saved-sql-file" ||
    isGroupLabel(props.node)
  );
});
const columnComment = computed(() =>
  props.node.type === "column" && props.node.meta && "comment" in props.node.meta
    ? (props.node.meta as any).comment
    : null,
);
const tableComment = computed(() =>
  (props.node.type === "table" || props.node.type === "view" || props.node.type === "mongo-collection") &&
  props.node.comment
    ? props.node.comment
    : null,
);
const paddingLeft = computed(() => treeItemPaddingLeft(props.depth));
const isConnected = computed(
  () =>
    props.node.type === "connection" &&
    !!props.node.connectionId &&
    connectionStore.connectedIds.has(props.node.connectionId),
);
const canConfigureVisibleDatabases = computed(() => {
  if (props.node.type !== "connection" || !props.node.connectionId) return false;
  return connectionStore.getConfig(props.node.connectionId)?.db_type !== "elasticsearch";
});

function connectionIconType(connectionId?: string) {
  const config = connectionId ? connectionStore.getConfig(connectionId) : undefined;
  return config?.driver_profile || config?.db_type || "postgres";
}

const connectionColor = computed(() => {
  const connectionId = props.node.connectionId;
  return connectionId ? connectionStore.getConfig(connectionId)?.color || "" : "";
});
const isActiveConnectionScope = computed(
  () => !!props.node.connectionId && connectionStore.activeConnectionId === props.node.connectionId,
);
const isSelected = computed(() => connectionStore.selectedTreeNodeId === props.node.id);
const rowStyle = computed(() => {
  const color = connectionColor.value;
  return {
    paddingLeft: paddingLeft.value,
    backgroundColor: hexToRgba(color, isActiveConnectionScope.value ? 0.14 : 0.08),
  };
});

function togglePin() {
  connectionStore.toggleTreeNodePin(props.node.id);
}

function openVisibleDatabasesDialog() {
  showVisibleDatabasesDialog.value = true;
}

// --- Connection Group Management ---
const isRenamingGroup = ref(false);
const renameInput = ref("");
const renameInputRef = ref<HTMLInputElement>();
const preventRenameCloseAutoFocus = ref(false);

function startRenameGroup() {
  renameInput.value = props.node.label;
  isRenamingGroup.value = true;
  preventRenameCloseAutoFocus.value = true;
  emit("rename-started");
  nextTick(() => {
    focusSidebarRenameInput(() => (isRenamingGroup.value ? renameInputRef.value : undefined));
  });
}

function onContextMenuCloseAutoFocus(event: Event) {
  if (shouldPreventRenameCloseAutoFocus(preventRenameCloseAutoFocus)) {
    event.preventDefault();
  }
}

watch(
  () => props.pendingRename,
  (val) => {
    if (val && props.node.type === "connection-group") {
      startRenameGroup();
    }
  },
  { immediate: true },
);

function finishRenameGroup() {
  isRenamingGroup.value = false;
  const trimmed = renameInput.value.trim();
  if (!trimmed) {
    connectionStore.deleteConnectionGroup(props.node.id);
    return;
  }
  if (trimmed !== props.node.label) {
    connectionStore.renameConnectionGroup(props.node.id, trimmed);
  }
}

function deleteConnectionGroup() {
  showDeleteGroupConfirm.value = true;
}

function newConnectionInGroup() {
  connectionStore.startCreatingConnectionInGroup(props.node.id);
}

function confirmDeleteGroup() {
  connectionStore.deleteConnectionGroup(props.node.id);
  showDeleteGroupConfirm.value = false;
  toast(t("connection.groupDeleted"), 2000);
}

const showDeleteGroupConfirm = ref(false);

function moveToGroup(groupId: string | null) {
  if (props.node.connectionId) {
    connectionStore.moveConnectionToGroup(props.node.connectionId, groupId);
  }
}

const showMoveToNewGroupDialog = ref(false);
const moveToNewGroupName = ref("");

function moveToNewGroup() {
  moveToNewGroupName.value = "";
  showMoveToNewGroupDialog.value = true;
}

function confirmMoveToNewGroup() {
  const name = moveToNewGroupName.value.trim();
  if (name && props.node.connectionId) {
    const groupId = connectionStore.createConnectionGroup(name);
    connectionStore.moveConnectionToGroup(props.node.connectionId, groupId);
  }
  showMoveToNewGroupDialog.value = false;
}

const availableGroups = computed(() => connectionStore.sidebarLayout.groups);

const currentGroupId = computed(() => {
  if (props.node.type !== "connection" || !props.node.connectionId) return null;
  for (const entry of connectionStore.sidebarLayout.order) {
    if (entry.type === "group" && entry.connectionIds.includes(props.node.connectionId)) {
      return entry.id;
    }
  }
  return null;
});

// --- Saved SQL Library ---
const showSavedSqlNameDialog = ref(false);
const savedSqlNameMode = ref<"folder-create" | "folder-rename" | "file-rename" | null>(null);
const savedSqlNameInput = ref("");
const showDeleteSavedSqlFileConfirm = ref(false);
const showDeleteSavedSqlFolderConfirm = ref(false);

function openCreateSavedSqlFolder() {
  savedSqlNameMode.value = "folder-create";
  savedSqlNameInput.value = t("savedSql.newFolderDefault");
  showSavedSqlNameDialog.value = true;
}

function openRenameSavedSqlFolder() {
  savedSqlNameMode.value = "folder-rename";
  savedSqlNameInput.value = props.node.label;
  showSavedSqlNameDialog.value = true;
}

function openRenameSavedSqlFile() {
  savedSqlNameMode.value = "file-rename";
  savedSqlNameInput.value = props.node.label.replace(/\.sql$/i, "");
  showSavedSqlNameDialog.value = true;
}

async function confirmSavedSqlName() {
  const name = savedSqlNameInput.value.trim();
  if (!name || !props.node.connectionId || !savedSqlNameMode.value) return;

  if (savedSqlNameMode.value === "folder-create") {
    await savedSqlStore.createFolder(props.node.connectionId, name);
  } else if (savedSqlNameMode.value === "folder-rename" && props.node.savedSqlFolderId) {
    await savedSqlStore.renameFolder(props.node.savedSqlFolderId, name);
  } else if (savedSqlNameMode.value === "file-rename" && props.node.savedSqlId) {
    await savedSqlStore.renameFile(props.node.savedSqlId, name.endsWith(".sql") ? name : `${name}.sql`);
  }

  connectionStore.refreshSavedSqlTree(props.node.connectionId);
  showSavedSqlNameDialog.value = false;
  savedSqlNameMode.value = null;
}

function deleteSavedSqlFile() {
  showDeleteSavedSqlFileConfirm.value = true;
}

async function confirmDeleteSavedSqlFile() {
  if (!props.node.savedSqlId) return;
  await savedSqlStore.deleteFile(props.node.savedSqlId);
  connectionStore.refreshSavedSqlTree(props.node.connectionId);
  showDeleteSavedSqlFileConfirm.value = false;
}

function deleteSavedSqlFolder() {
  showDeleteSavedSqlFolderConfirm.value = true;
}

async function confirmDeleteSavedSqlFolder() {
  if (!props.node.savedSqlFolderId) return;
  await savedSqlStore.deleteFolder(props.node.savedSqlFolderId);
  connectionStore.refreshSavedSqlTree(props.node.connectionId);
  showDeleteSavedSqlFolderConfirm.value = false;
}

// --- Drag and Drop ---
import { useDragSort } from "@/composables/useDragSort";

const {
  state: dragState,
  startDrag,
  updateTarget,
  clearTarget,
} = useDragSort((draggedId, targetId, position) => connectionStore.reorderSidebarEntry(draggedId, targetId, position));

const isDraggable = computed(() => {
  if (props.dragDisabled) return false;
  return props.node.type === "connection" || props.node.type === "connection-group";
});

const isDropTarget = computed(() => props.node.type === "connection" || props.node.type === "connection-group");

const showDropBefore = computed(
  () => dragState.active && dragState.targetId === props.node.id && dragState.dropPosition === "before",
);
const showDropAfter = computed(
  () => dragState.active && dragState.targetId === props.node.id && dragState.dropPosition === "after",
);
const showDropInside = computed(
  () => dragState.active && dragState.targetId === props.node.id && dragState.dropPosition === "inside",
);
const isDragging = computed(() => dragState.active && dragState.draggedId === props.node.id);
</script>

<template>
  <ContextMenu>
    <ContextMenuTrigger as-child>
      <div>
        <div
          ref="rowRef"
          class="group flex min-w-0 items-center gap-1.5 py-1 px-2 cursor-pointer hover:bg-accent transition-colors relative outline-none"
          style="contain: layout style paint"
          :class="{
            'ring-1 ring-primary/50 bg-primary/5': showDropInside,
            'opacity-50': isDragging,
            'rounded-none': connectionColor && !isSelected,
            'rounded-sm': !connectionColor && !isSelected,
            'tree-item-active rounded-md': isSelected,
          }"
          :tabindex="isSelected ? 0 : -1"
          :style="rowStyle"
          @click="onClick"
          @dblclick="onDoubleClick"
          @keydown="onKeydown"
          @mousedown="isDraggable ? startDrag($event, node.id, node.type) : undefined"
          @mousemove="isDropTarget ? updateTarget($event, node.id, node.type) : undefined"
          @mouseleave="clearTarget(node.id)"
        >
          <div
            v-if="showDropBefore"
            class="absolute right-2 top-0 h-0.5 bg-primary rounded-full pointer-events-none"
            :style="{ left: paddingLeft }"
          />
          <div
            v-if="showDropAfter"
            class="absolute right-2 bottom-0 h-0.5 bg-primary rounded-full pointer-events-none"
            :style="{ left: paddingLeft }"
          />
          <template v-if="canExpand">
            <button
              type="button"
              class="-m-0.5 flex h-4 w-4 shrink-0 items-center justify-center rounded-sm text-muted-foreground hover:bg-muted hover:text-foreground"
              @click.stop="toggle"
            >
              <Loader2 v-if="node.isLoading" class="w-3.5 h-3.5 animate-spin" />
              <ChevronDown v-else-if="node.isExpanded" class="w-3.5 h-3.5" />
              <ChevronRight v-else class="w-3.5 h-3.5" />
            </button>
          </template>
          <span v-else class="w-3.5 h-3.5 shrink-0" />
          <DatabaseIcon
            v-if="node.type === 'connection'"
            :db-type="connectionIconType(node.connectionId)"
            class="w-3.5 h-3.5 shrink-0"
          />
          <component
            v-else
            :is="getIconInfo(node)?.icon || Database"
            class="w-3.5 h-3.5 shrink-0"
            :class="getIconInfo(node)?.colorClass"
          />
          <input
            v-if="isRenamingGroup"
            ref="renameInputRef"
            v-model="renameInput"
            class="min-w-0 flex-1 truncate bg-transparent border border-primary/50 rounded px-1 outline-none"
            @blur="finishRenameGroup"
            @keydown.enter.prevent="finishRenameGroup"
            @keydown.escape.prevent="isRenamingGroup = false"
            @click.stop
          />
          <Tooltip v-else :disabled="isTooltipDisabled(node)">
            <TooltipTrigger as-child>
              <span ref="labelRef" class="min-w-0 flex-1 truncate">{{ visibleLabel(node) }}</span>
            </TooltipTrigger>
            <TooltipContent side="right" :side-offset="8">{{ displayLabel(node) }}</TooltipContent>
          </Tooltip>
          <span
            v-if="
              (node.type === 'group-tables' ||
                node.type === 'group-views' ||
                node.type === 'group-procedures' ||
                node.type === 'group-functions') &&
              node.objectCount != null
            "
            class="text-muted-foreground text-[10px] shrink-0"
            >{{ node.objectCount }}</span
          >
          <Badge v-if="isNodeDefaultDatabase" variant="secondary" class="h-4 px-1.5 text-[10px]">
            {{ t("editor.defaultDatabase") }}
          </Badge>
          <span v-if="columnComment" class="truncate text-muted-foreground/60 text-[10px] max-w-[40%]">{{
            columnComment
          }}</span>
          <span
            v-if="tableComment"
            class="truncate text-muted-foreground/60 text-[10px] max-w-[25%] group-hover:hidden"
            :title="tableComment"
            >{{ tableComment }}</span
          >
          <span
            v-if="
              node.type === 'connection' && node.connectionId && connectionStore.connectedIds.has(node.connectionId)
            "
            class="w-1.5 h-1.5 rounded-full bg-green-500 shrink-0"
          />
          <ConnectionErrorIndicator
            v-if="node.type === 'connection'"
            :connection-id="node.connectionId"
            trigger-class="h-4 w-4"
          />
          <button
            v-if="canPin"
            class="rounded p-0.5 text-muted-foreground hover:bg-muted-foreground/15 hover:text-foreground focus:opacity-100"
            :class="isPinned ? 'opacity-100 text-primary' : 'opacity-0 group-hover:opacity-100'"
            :title="isPinned ? t('contextMenu.unpin') : t('contextMenu.pin')"
            @click.stop="togglePin"
          >
            <Pin class="w-3 h-3" :class="{ 'fill-current': isPinned }" />
          </button>
        </div>
      </div>
    </ContextMenuTrigger>

    <ContextMenuContent class="w-auto min-w-36" @close-auto-focus="onContextMenuCloseAutoFocus">
      <ContextMenuItem v-if="canPin" @click="togglePin">
        <Pin class="w-4 h-4" :class="{ 'fill-current': isPinned }" />
        {{ isPinned ? t("contextMenu.unpin") : t("contextMenu.pin") }}
      </ContextMenuItem>
      <ContextMenuSeparator v-if="canPin && hasTypeMenu" />

      <template v-if="node.type === 'connection'">
        <ContextMenuItem v-if="!isConnected" @click="toggle">
          <Plug class="w-4 h-4" /> {{ t("contextMenu.openConnection") }}
        </ContextMenuItem>
        <ContextMenuItem v-else @click="disconnectConnection">
          <Unplug class="w-4 h-4" /> {{ t("contextMenu.closeConnection") }}
        </ContextMenuItem>
        <ContextMenuItem @click="newQuery">
          <TerminalSquare class="w-4 h-4" /> {{ t("contextMenu.newQuery") }}
        </ContextMenuItem>
        <ContextMenuItem v-if="canOpenSqlFileExecution" @click="openSqlFileExecution">
          <FileCode class="w-4 h-4" /> {{ t("sqlFile.title") }}
        </ContextMenuItem>
        <ContextMenuItem v-if="canCreateDatabase" @click="openCreateDatabase">
          <Plus class="w-4 h-4" />
          {{ isDuckDbConnection ? t("contextMenu.createDuckDbFile") : t("contextMenu.createDatabase") }}
        </ContextMenuItem>
        <ContextMenuSeparator />
        <ContextMenuSub v-if="availableGroups.length > 0 || currentGroupId">
          <ContextMenuSubTrigger>
            <FolderInput class="w-4 h-4" /> {{ t("connectionGroup.moveToGroup") }}
          </ContextMenuSubTrigger>
          <ContextMenuSubContent>
            <ContextMenuItem
              v-for="group in availableGroups"
              :key="group.id"
              :disabled="group.id === currentGroupId"
              @click="moveToGroup(group.id)"
            >
              <FolderOpen class="w-4 h-4" /> {{ group.name }}
            </ContextMenuItem>
            <ContextMenuSeparator v-if="currentGroupId" />
            <ContextMenuItem v-if="currentGroupId" @click="moveToGroup(null)">
              {{ t("connectionGroup.ungrouped") }}
            </ContextMenuItem>
            <ContextMenuSeparator />
            <ContextMenuItem @click="moveToNewGroup">
              <FolderPlus class="w-4 h-4" /> {{ t("connectionGroup.newGroup") }}
            </ContextMenuItem>
          </ContextMenuSubContent>
        </ContextMenuSub>
        <ContextMenuItem v-else @click="moveToNewGroup">
          <FolderPlus class="w-4 h-4" /> {{ t("connectionGroup.moveToNewGroup") }}
        </ContextMenuItem>
        <ContextMenuItem @click="refresh">
          <RefreshCw class="w-4 h-4" /> {{ t("contextMenu.refreshChildren") }}
        </ContextMenuItem>
        <ContextMenuItem v-if="canConfigureVisibleDatabases" @click="openVisibleDatabasesDialog">
          <ListFilter class="w-4 h-4" /> {{ t("contextMenu.selectVisibleDatabases") }}
        </ContextMenuItem>
        <ContextMenuItem @click="editConnection">
          <Pencil class="w-4 h-4" /> {{ t("contextMenu.editConnection") }}
        </ContextMenuItem>
        <ContextMenuItem @click="duplicateConnection">
          <CopyPlus class="w-4 h-4" /> {{ t("contextMenu.duplicateConnection") }}
        </ContextMenuItem>
        <ContextMenuSeparator />
        <ContextMenuItem class="text-destructive" @click="deleteConnection">
          <Trash2 class="w-4 h-4" /> {{ t("contextMenu.deleteConnection") }}
        </ContextMenuItem>
      </template>

      <template v-if="node.type === 'connection-group'">
        <ContextMenuItem @click="newConnectionInGroup">
          <Plus class="w-4 h-4" /> {{ t("toolbar.newConnection") }}
        </ContextMenuItem>
        <ContextMenuSeparator />
        <ContextMenuItem @select="startRenameGroup">
          <Pencil class="w-4 h-4" /> {{ t("connectionGroup.renameGroup") }}
        </ContextMenuItem>
        <ContextMenuSeparator />
        <ContextMenuItem class="text-destructive" @click="deleteConnectionGroup">
          <Trash2 class="w-4 h-4" /> {{ t("connectionGroup.deleteGroup") }}
        </ContextMenuItem>
      </template>

      <template v-if="node.type === 'database' || node.type === 'schema'">
        <ContextMenuItem v-if="canOpenObjectBrowser" @click="openObjectBrowser">
          <TableProperties class="w-4 h-4" /> {{ t("contextMenu.openObjectBrowser") }}
        </ContextMenuItem>
        <ContextMenuItem @click="newQuery">
          <TerminalSquare class="w-4 h-4" /> {{ t("contextMenu.newQuery") }}
        </ContextMenuItem>
        <ContextMenuItem v-if="node.type === 'database' && !isNodeDefaultDatabase" @click="setNodeAsDefaultDatabase">
          <Database class="w-4 h-4" /> {{ t("contextMenu.setDefaultDatabase") }}
        </ContextMenuItem>
        <ContextMenuItem v-if="node.type === 'database' && isNodeDefaultDatabase" @click="clearNodeDefaultDatabase">
          <Database class="w-4 h-4" /> {{ t("contextMenu.clearDefaultDatabase") }}
        </ContextMenuItem>
        <ContextMenuItem v-if="canCreateTable" @click="createTable">
          <Plus class="w-4 h-4" /> {{ t("contextMenu.createTable") }}
        </ContextMenuItem>
        <ContextMenuItem v-if="canCreateSchema" @click="openCreateSchemaDialog">
          <Plus class="w-4 h-4" /> {{ t("contextMenu.createSchema") }}
        </ContextMenuItem>
        <ContextMenuItem v-if="canOpenSqlFileExecution" @click="openSqlFileExecution">
          <FileCode class="w-4 h-4" /> {{ t("sqlFile.title") }}
        </ContextMenuItem>
        <ContextMenuItem v-if="canOpenDiagram" @click="openDiagram">
          <Network class="w-4 h-4" /> {{ t("diagram.open") }}
        </ContextMenuItem>
        <ContextMenuItem v-if="canOpenDatabaseSearch" @click="openDatabaseSearch">
          <Search class="w-4 h-4" /> {{ t("databaseSearch.open") }}
        </ContextMenuItem>
        <ContextMenuItem @click="refresh">
          <RefreshCw class="w-4 h-4" /> {{ t("contextMenu.refreshChildren") }}
        </ContextMenuItem>
        <ContextMenuSeparator />
        <ContextMenuItem @click="openTransfer">
          <ArrowRightLeft class="w-4 h-4" /> {{ t("transfer.dataTransfer") }}
        </ContextMenuItem>
        <ContextMenuItem @click="openSchemaDiff">
          <ArrowRightLeft class="w-4 h-4" /> {{ t("diff.title") }}
        </ContextMenuItem>
        <ContextMenuItem @click="openDataCompare">
          <ArrowRightLeft class="w-4 h-4" /> {{ t("dataCompare.title") }}
        </ContextMenuItem>
        <ContextMenuItem @click="openDatabaseExport">
          <Download class="w-4 h-4" />
          {{ t("contextMenu.exportDatabase") }}
        </ContextMenuItem>
        <template v-if="canDropDatabase || canDropSchema">
          <ContextMenuSeparator />
          <ContextMenuItem v-if="canDropDatabase" class="text-destructive" @click="dropDatabase">
            <Trash2 class="w-4 h-4" /> {{ t("contextMenu.dropDatabase") }}
          </ContextMenuItem>
          <ContextMenuItem v-if="canDropSchema" class="text-destructive" @click="dropSchema">
            <Trash2 class="w-4 h-4" /> {{ t("contextMenu.dropSchema") }}
          </ContextMenuItem>
        </template>
      </template>

      <template v-if="node.type === 'redis-db' || node.type === 'mongo-db'">
        <ContextMenuItem @click="newQuery">
          <TerminalSquare class="w-4 h-4" /> {{ t("contextMenu.newQuery") }}
        </ContextMenuItem>
        <ContextMenuItem v-if="!isNodeDefaultDatabase" @click="setNodeAsDefaultDatabase">
          <Database class="w-4 h-4" /> {{ t("contextMenu.setDefaultDatabase") }}
        </ContextMenuItem>
        <ContextMenuItem v-if="isNodeDefaultDatabase" @click="clearNodeDefaultDatabase">
          <Database class="w-4 h-4" /> {{ t("contextMenu.clearDefaultDatabase") }}
        </ContextMenuItem>
        <template v-if="node.type === 'redis-db'">
          <ContextMenuSeparator />
          <ContextMenuItem class="text-destructive" @click="flushRedisDb">
            <Eraser class="w-4 h-4" /> {{ t("redis.flushDb") }}
          </ContextMenuItem>
        </template>
      </template>

      <template v-if="node.type === 'table' || node.type === 'view'">
        <ContextMenuItem @click="copyName"> <Copy class="w-4 h-4" /> {{ t("contextMenu.copyName") }} </ContextMenuItem>
        <ContextMenuSeparator />
        <ContextMenuItem @click="openData">
          <TableProperties class="w-4 h-4" /> {{ t("contextMenu.viewData") }}
        </ContextMenuItem>
        <ContextMenuItem v-if="node.type === 'view'" @click="viewObjectSource">
          <Code2 class="w-4 h-4" /> {{ t("contextMenu.viewSource") }}
        </ContextMenuItem>
        <ContextMenuItem v-if="node.type === 'view'" @click="viewObjectDdl">
          <FileCode class="w-4 h-4" /> {{ t("contextMenu.viewDdl") }}
        </ContextMenuItem>
        <ContextMenuItem v-if="canOpenStructureEditor" @click="openStructureEditor">
          <PencilRuler class="w-4 h-4" /> {{ t("contextMenu.editStructure") }}
        </ContextMenuItem>
        <ContextMenuItem v-if="canRenameObject" @click="openRenameObjectDialog">
          <Pencil class="w-4 h-4" /> {{ t("contextMenu.renameObject") }}
        </ContextMenuItem>
        <ContextMenuItem @click="newQuery">
          <TerminalSquare class="w-4 h-4" /> {{ t("contextMenu.newQuery") }}
        </ContextMenuItem>
        <ContextMenuItem v-if="canOpenDiagram" @click="openDiagram">
          <Network class="w-4 h-4" /> {{ t("diagram.open") }}
        </ContextMenuItem>
        <ContextMenuItem v-if="canOpenTableImport" @click="openTableImport">
          <FileUp class="w-4 h-4" /> {{ t("contextMenu.importData") }}
        </ContextMenuItem>
        <ContextMenuItem v-if="isTableNotView" @click="openDataCompare">
          <ArrowRightLeft class="w-4 h-4" /> {{ t("dataCompare.title") }}
        </ContextMenuItem>
        <ContextMenuSeparator />
        <ContextMenuSub>
          <ContextMenuSubTrigger>
            <Download class="w-4 h-4" /> {{ t("contextMenu.exportData") }}
          </ContextMenuSubTrigger>
          <ContextMenuSubContent>
            <ContextMenuItem @click="exportData('csv')">CSV</ContextMenuItem>
            <ContextMenuItem @click="exportData('json')">JSON</ContextMenuItem>
            <ContextMenuItem @click="exportData('sql')">SQL INSERT</ContextMenuItem>
            <ContextMenuItem @click="exportDataXlsx">XLSX</ContextMenuItem>
          </ContextMenuSubContent>
        </ContextMenuSub>
        <ContextMenuItem @click="openDatabaseExport">
          <Download class="w-4 h-4" />
          {{ t("contextMenu.exportDatabase") }}
        </ContextMenuItem>
        <ContextMenuItem @click="exportStructure">
          <FileCode class="w-4 h-4" /> {{ t("contextMenu.exportStructure") }}
        </ContextMenuItem>
        <template v-if="isTableNotView">
          <ContextMenuSeparator />
          <ContextMenuItem @click="duplicateStructure">
            <CopyPlus class="w-4 h-4" /> {{ t("contextMenu.duplicateStructure") }}
          </ContextMenuItem>
          <ContextMenuSeparator />
          <ContextMenuItem v-if="supportsTruncate" class="text-destructive" @click="truncateTable">
            <Scissors class="w-4 h-4" /> {{ t("contextMenu.truncateTable") }}
          </ContextMenuItem>
          <ContextMenuItem class="text-destructive" @click="emptyTable">
            <Eraser class="w-4 h-4" /> {{ t("contextMenu.emptyTable") }}
          </ContextMenuItem>
          <ContextMenuItem class="text-destructive" @click="dropTable">
            <Trash2 class="w-4 h-4" /> {{ t("contextMenu.dropTable") }}
          </ContextMenuItem>
        </template>
        <ContextMenuSeparator />
        <ContextMenuItem @click="refresh">
          <RefreshCw class="w-4 h-4" /> {{ t("contextMenu.refreshChildren") }}
        </ContextMenuItem>
      </template>

      <template v-if="node.type === 'column'">
        <ContextMenuItem v-if="canOpenFieldLineage" @click="openFieldLineage">
          <Network class="w-4 h-4" /> {{ t("lineage.open") }}
        </ContextMenuItem>
      </template>

      <template v-if="node.type === 'procedure' || node.type === 'function'">
        <ContextMenuItem @click="viewObjectSource">
          <Code2 class="w-4 h-4 mr-2" />
          {{ t("contextMenu.viewSource") }}
        </ContextMenuItem>
        <ContextMenuItem v-if="canRenameObject" @click="openRenameObjectDialog">
          <Pencil class="w-4 h-4 mr-2" />
          {{ t("contextMenu.renameObject") }}
        </ContextMenuItem>
        <ContextMenuSeparator />
        <ContextMenuItem class="text-destructive" @click="requestDropObject">
          <Trash2 class="w-4 h-4 mr-2" />
          {{ node.type === "procedure" ? t("contextMenu.dropProcedure") : t("contextMenu.dropFunction") }}
        </ContextMenuItem>
      </template>

      <template v-if="isGroupLabel(node)">
        <ContextMenuItem v-if="node.type === 'saved-sql-root'" @click="openCreateSavedSqlFolder">
          <FolderPlus class="w-4 h-4" /> {{ t("savedSql.newFolder") }}
        </ContextMenuItem>
        <template v-if="node.type === 'saved-sql-folder'">
          <ContextMenuItem @click="openRenameSavedSqlFolder">
            <Pencil class="w-4 h-4" /> {{ t("savedSql.renameFolder") }}
          </ContextMenuItem>
          <ContextMenuItem class="text-destructive" @click="deleteSavedSqlFolder">
            <Trash2 class="w-4 h-4" /> {{ t("savedSql.deleteFolder") }}
          </ContextMenuItem>
          <ContextMenuSeparator />
        </template>
        <ContextMenuItem v-if="node.type !== 'saved-sql-root' && node.type !== 'saved-sql-folder'" @click="refresh">
          <RefreshCw class="w-4 h-4" /> {{ t("contextMenu.refreshChildren") }}
        </ContextMenuItem>
      </template>

      <template v-if="node.type === 'saved-sql-file'">
        <ContextMenuItem @click="openSavedSqlFile">
          <FileText class="w-4 h-4" /> {{ t("savedSql.open") }}
        </ContextMenuItem>
        <ContextMenuItem @click="openRenameSavedSqlFile">
          <Pencil class="w-4 h-4" /> {{ t("savedSql.renameFile") }}
        </ContextMenuItem>
        <ContextMenuSeparator />
        <ContextMenuItem class="text-destructive" @click="deleteSavedSqlFile">
          <Trash2 class="w-4 h-4" /> {{ t("savedSql.deleteFile") }}
        </ContextMenuItem>
      </template>

      <template v-if="node.type !== 'connection'">
        <ContextMenuSeparator v-if="hasTypeMenu" />
        <ContextMenuItem @click="copyName"> <Copy class="w-4 h-4" /> {{ t("contextMenu.copyName") }} </ContextMenuItem>
      </template>
    </ContextMenuContent>
  </ContextMenu>

  <VisibleDatabasesDialog
    v-if="node.type === 'connection' && node.connectionId"
    v-model:open="showVisibleDatabasesDialog"
    :connection-id="node.connectionId"
    :connection-name="node.label"
  />

  <Dialog v-model:open="showDeleteConfirm">
    <DialogContent class="sm:max-w-[400px]">
      <DialogHeader>
        <DialogTitle>{{ t("contextMenu.confirmDeleteTitle") }}</DialogTitle>
      </DialogHeader>
      <p class="text-sm text-muted-foreground">
        {{ t("contextMenu.confirmDeleteMessage", { name: node.label }) }}
      </p>
      <DialogFooter>
        <Button variant="outline" @click="showDeleteConfirm = false">{{ t("dangerDialog.cancel") }}</Button>
        <Button
          variant="destructive"
          @click="
            showDeleteConfirm = false;
            confirmDelete();
          "
          >{{ t("contextMenu.deleteConnection") }}</Button
        >
      </DialogFooter>
    </DialogContent>
  </Dialog>

  <Dialog v-model:open="showMoveToNewGroupDialog">
    <DialogContent class="sm:max-w-[360px]">
      <DialogHeader>
        <DialogTitle>{{ t("connectionGroup.createGroup") }}</DialogTitle>
      </DialogHeader>
      <Input
        v-model="moveToNewGroupName"
        :placeholder="t('connectionGroup.groupNamePlaceholder')"
        @keydown.enter.prevent="confirmMoveToNewGroup"
      />
      <DialogFooter>
        <Button variant="outline" @click="showMoveToNewGroupDialog = false">{{ t("dangerDialog.cancel") }}</Button>
        <Button :disabled="!moveToNewGroupName.trim()" @click="confirmMoveToNewGroup">{{
          t("connectionGroup.createGroup")
        }}</Button>
      </DialogFooter>
    </DialogContent>
  </Dialog>

  <Dialog v-model:open="showDeleteGroupConfirm">
    <DialogContent class="sm:max-w-[400px]">
      <DialogHeader>
        <DialogTitle>{{ t("connectionGroup.deleteGroupConfirmTitle") }}</DialogTitle>
      </DialogHeader>
      <p class="text-sm text-muted-foreground">
        {{ t("connectionGroup.deleteGroupConfirmMessage", { name: node.label }) }}
      </p>
      <DialogFooter>
        <Button variant="outline" @click="showDeleteGroupConfirm = false">{{ t("dangerDialog.cancel") }}</Button>
        <Button variant="destructive" @click="confirmDeleteGroup">{{ t("connectionGroup.deleteGroup") }}</Button>
      </DialogFooter>
    </DialogContent>
  </Dialog>

  <Dialog v-model:open="showSavedSqlNameDialog">
    <DialogContent class="sm:max-w-[380px]">
      <DialogHeader>
        <DialogTitle>
          {{
            savedSqlNameMode === "folder-create"
              ? t("savedSql.newFolder")
              : savedSqlNameMode === "folder-rename"
                ? t("savedSql.renameFolder")
                : t("savedSql.renameFile")
          }}
        </DialogTitle>
      </DialogHeader>
      <Input v-model="savedSqlNameInput" @keydown.enter.prevent="confirmSavedSqlName" />
      <DialogFooter>
        <Button variant="outline" @click="showSavedSqlNameDialog = false">{{ t("dangerDialog.cancel") }}</Button>
        <Button :disabled="!savedSqlNameInput.trim()" @click="confirmSavedSqlName">{{
          t("dangerDialog.confirm")
        }}</Button>
      </DialogFooter>
    </DialogContent>
  </Dialog>

  <Dialog v-model:open="showRenameObjectDialog">
    <DialogContent class="sm:max-w-[420px]">
      <DialogHeader>
        <DialogTitle>{{ t("contextMenu.renameObjectTitle") }}</DialogTitle>
      </DialogHeader>
      <div class="grid gap-3">
        <Input
          v-model="renameObjectName"
          :placeholder="t('contextMenu.renameObjectNamePlaceholder')"
          @keydown.enter.prevent="confirmRenameObject"
        />
        <pre
          v-if="renameObjectPreviewSql"
          class="max-h-32 overflow-auto rounded bg-muted p-3 text-xs whitespace-pre-wrap"
          v-html="highlight(renameObjectPreviewSql)"
        ></pre>
        <p v-if="renameObjectError" class="text-sm text-destructive">{{ renameObjectError }}</p>
      </div>
      <DialogFooter>
        <Button variant="outline" @click="showRenameObjectDialog = false">{{ t("dangerDialog.cancel") }}</Button>
        <Button
          :disabled="!renameObjectName.trim() || renameObjectName.trim() === node.label"
          @click="confirmRenameObject"
        >
          {{ t("contextMenu.renameObject") }}
        </Button>
      </DialogFooter>
    </DialogContent>
  </Dialog>

  <Dialog v-model:open="showDeleteSavedSqlFileConfirm">
    <DialogContent class="sm:max-w-[400px]">
      <DialogHeader>
        <DialogTitle>{{ t("savedSql.deleteFile") }}</DialogTitle>
      </DialogHeader>
      <p class="text-sm text-muted-foreground">
        {{ t("savedSql.deleteFileConfirm", { name: node.label }) }}
      </p>
      <DialogFooter>
        <Button variant="outline" @click="showDeleteSavedSqlFileConfirm = false">{{ t("dangerDialog.cancel") }}</Button>
        <Button variant="destructive" @click="confirmDeleteSavedSqlFile">{{ t("savedSql.deleteFile") }}</Button>
      </DialogFooter>
    </DialogContent>
  </Dialog>

  <Dialog v-model:open="showDeleteSavedSqlFolderConfirm">
    <DialogContent class="sm:max-w-[400px]">
      <DialogHeader>
        <DialogTitle>{{ t("savedSql.deleteFolder") }}</DialogTitle>
      </DialogHeader>
      <p class="text-sm text-muted-foreground">
        {{ t("savedSql.deleteFolderConfirm", { name: node.label }) }}
      </p>
      <DialogFooter>
        <Button variant="outline" @click="showDeleteSavedSqlFolderConfirm = false">{{
          t("dangerDialog.cancel")
        }}</Button>
        <Button variant="destructive" @click="confirmDeleteSavedSqlFolder">{{ t("savedSql.deleteFolder") }}</Button>
      </DialogFooter>
    </DialogContent>
  </Dialog>

  <DangerConfirmDialog
    v-model:open="showDropTableConfirm"
    :title="t('contextMenu.confirmDropTableTitle')"
    :message="t('contextMenu.confirmDropTableMessage', { name: node.label })"
    :sql="dropTablePreviewSql"
    :confirm-label="t('contextMenu.dropTable')"
    @confirm="confirmDropTable"
  />

  <DangerConfirmDialog
    v-model:open="showEmptyTableConfirm"
    :title="t('contextMenu.confirmEmptyTableTitle')"
    :message="t('contextMenu.confirmEmptyTableMessage', { name: node.label })"
    :sql="emptyTablePreviewSql"
    :confirm-label="t('contextMenu.emptyTable')"
    @confirm="confirmEmptyTable"
  />

  <DangerConfirmDialog
    v-model:open="showTruncateTableConfirm"
    :title="t('contextMenu.confirmTruncateTableTitle')"
    :message="t('contextMenu.confirmTruncateTableMessage', { name: node.label })"
    :sql="truncateTablePreviewSql"
    :confirm-label="t('contextMenu.truncateTable')"
    @confirm="confirmTruncateTable"
  />

  <DangerConfirmDialog
    v-model:open="showDropObjectConfirm"
    :title="
      node.type === 'procedure' ? t('contextMenu.confirmDropProcedureTitle') : t('contextMenu.confirmDropFunctionTitle')
    "
    :message="
      node.type === 'procedure'
        ? t('contextMenu.confirmDropProcedureMessage', { name: node.label })
        : t('contextMenu.confirmDropFunctionMessage', { name: node.label })
    "
    :sql="dropObjectPreviewSql"
    :confirm-label="node.type === 'procedure' ? t('contextMenu.dropProcedure') : t('contextMenu.dropFunction')"
    @confirm="confirmDropObject"
  />

  <Dialog v-model:open="showDuplicateDialog">
    <DialogContent class="sm:max-w-[400px]">
      <DialogHeader>
        <DialogTitle>{{ t("contextMenu.duplicateNameTitle") }}</DialogTitle>
      </DialogHeader>
      <Input
        v-model="duplicateTableName"
        :placeholder="t('contextMenu.duplicateNamePlaceholder')"
        @keydown.enter.prevent="confirmDuplicateStructure"
      />
      <DialogFooter>
        <Button variant="outline" @click="showDuplicateDialog = false">{{ t("dangerDialog.cancel") }}</Button>
        <Button :disabled="!duplicateTableName.trim()" @click="confirmDuplicateStructure">{{
          t("dangerDialog.confirm")
        }}</Button>
      </DialogFooter>
    </DialogContent>
  </Dialog>

  <Dialog v-model:open="showCreateDatabaseDialog">
    <DialogContent class="sm:max-w-[400px]">
      <DialogHeader>
        <DialogTitle>{{ t("contextMenu.createDatabase") }}</DialogTitle>
      </DialogHeader>
      <Input
        v-model="createDatabaseName"
        :placeholder="t('contextMenu.createDatabaseNamePlaceholder')"
        @keydown.enter.prevent="confirmCreateDatabase"
      />
      <div v-if="canSetCreateDatabaseCharset" class="grid gap-2">
        <div class="grid gap-1.5">
          <label class="text-xs font-medium text-muted-foreground">{{ t("contextMenu.createDatabaseCharset") }}</label>
          <Input
            v-model="createDatabaseCharset"
            :placeholder="t('contextMenu.createDatabaseCharsetPlaceholder')"
            @keydown.enter.prevent="confirmCreateDatabase"
          />
        </div>
        <div class="grid gap-1.5">
          <label class="text-xs font-medium text-muted-foreground">{{
            t("contextMenu.createDatabaseCollation")
          }}</label>
          <Input
            v-model="createDatabaseCollation"
            :placeholder="t('contextMenu.createDatabaseCollationPlaceholder')"
            @keydown.enter.prevent="confirmCreateDatabase"
          />
        </div>
      </div>
      <DialogFooter>
        <Button variant="outline" @click="showCreateDatabaseDialog = false">{{ t("dangerDialog.cancel") }}</Button>
        <Button :disabled="!createDatabaseName.trim()" @click="confirmCreateDatabase">{{
          t("dangerDialog.confirm")
        }}</Button>
      </DialogFooter>
    </DialogContent>
  </Dialog>

  <DangerConfirmDialog
    v-model:open="showDropDatabaseConfirm"
    :title="t('contextMenu.confirmDropDatabaseTitle')"
    :message="t('contextMenu.confirmDropDatabaseMessage', { name: node.label })"
    :sql="dropDatabasePreviewSql"
    :confirm-label="t('contextMenu.dropDatabase')"
    @confirm="confirmDropDatabase"
  />

  <DangerConfirmDialog
    v-model:open="showFlushRedisDbConfirm"
    :title="t('redis.flushDb')"
    :message="t('redis.flushDbMessage')"
    :details="t('redis.flushDbDetails', { db: node.database })"
    :confirm-label="t('redis.flushDbConfirm')"
    @confirm="confirmFlushRedisDb"
  />

  <Dialog v-model:open="showCreateSchemaDialog">
    <DialogContent class="sm:max-w-[400px]">
      <DialogHeader>
        <DialogTitle>{{ t("contextMenu.createSchema") }}</DialogTitle>
      </DialogHeader>
      <Input
        v-model="createSchemaName"
        :placeholder="t('contextMenu.createSchemaNamePlaceholder')"
        @keydown.enter.prevent="confirmCreateSchema"
      />
      <DialogFooter>
        <Button variant="outline" @click="showCreateSchemaDialog = false">{{ t("dangerDialog.cancel") }}</Button>
        <Button :disabled="!createSchemaName.trim()" @click="confirmCreateSchema">{{
          t("dangerDialog.confirm")
        }}</Button>
      </DialogFooter>
    </DialogContent>
  </Dialog>

  <DangerConfirmDialog
    v-model:open="showDropSchemaConfirm"
    :title="t('contextMenu.confirmDropSchemaTitle')"
    :message="t('contextMenu.confirmDropSchemaMessage', { name: node.label })"
    :sql="dropSchemaPreviewSql"
    :confirm-label="t('contextMenu.dropSchema')"
    @confirm="confirmDropSchema"
  />
</template>

<style>
/* Unfocused: subtle gray */
.tree-item-active {
  background-color: oklch(0.94 0 0) !important;
}
:root.dark .tree-item-active {
  background-color: oklch(0.26 0 0) !important;
}

/* Focused: soft blue */
.sidebar-tree:focus-within .tree-item-active {
  background-color: oklch(0.91 0.03 250) !important;
}
:root.dark .sidebar-tree:focus-within .tree-item-active {
  background-color: oklch(0.35 0.06 250) !important;
}
</style>
