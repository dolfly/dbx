import { ref } from "vue";
import { useConnectionStore } from "@/stores/connectionStore";
import { filterDatabaseNamesForConnection } from "@/lib/visibleDatabases";
import * as api from "@/lib/api";

export function useDatabaseOptions() {
  const connectionStore = useConnectionStore();

  const databaseOptions = ref<Record<string, string[]>>({});
  const loadingDatabaseOptions = ref<Record<string, boolean>>({});

  async function loadDatabaseOptions(connectionId: string) {
    const connection = connectionStore.getConfig(connectionId);
    if (!connection || loadingDatabaseOptions.value[connectionId]) return;

    loadingDatabaseOptions.value[connectionId] = true;
    try {
      await connectionStore.ensureConnected(connectionId);
      if (connection.db_type === "redis") {
        const dbs = await api.redisListDatabases(connectionId);
        databaseOptions.value[connectionId] = dbs.map((db) => String(db.db));
      } else if (connection.db_type === "mongodb") {
        databaseOptions.value[connectionId] = filterDatabaseNamesForConnection(
          await api.mongoListDatabases(connectionId),
          connection,
        );
      } else {
        const dbs = await api.listDatabases(connectionId);
        databaseOptions.value[connectionId] = filterDatabaseNamesForConnection(
          dbs.map((db) => db.name),
          connection,
        );
      }
    } finally {
      loadingDatabaseOptions.value[connectionId] = false;
    }
  }

  async function getDatabaseOptions(connectionId: string): Promise<string[]> {
    if (!databaseOptions.value[connectionId]) {
      await loadDatabaseOptions(connectionId);
    }
    return databaseOptions.value[connectionId] ?? [];
  }

  return { databaseOptions, loadingDatabaseOptions, loadDatabaseOptions, getDatabaseOptions };
}
