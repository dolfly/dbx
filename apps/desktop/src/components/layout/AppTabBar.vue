<script setup lang="ts">
import { computed, ref, watch, nextTick } from "vue";
import type { CSSProperties } from "vue";
import { useI18n } from "vue-i18n";
import { X, Pin, ChevronDown, Table2, Code2, TableProperties, PencilRuler, Package, Check } from "lucide-vue-next";
import CustomContextMenu, { type ContextMenuItem } from "@/components/ui/CustomContextMenu.vue";
import LightDropdown from "@/components/ui/LightDropdown.vue";
import { Tooltip, TooltipTrigger, TooltipContent } from "@/components/ui/tooltip";
import { useQueryStore } from "@/stores/queryStore";
import { useSettingsStore } from "@/stores/settingsStore";
import { useTabScroll } from "@/composables/useTabScroll";
import { connectionColor, tabDisplayTitle, tabTooltipLines } from "@/lib/tabPresentation";
import { hexToRgba } from "@/lib/color";
import type { QueryTab } from "@/types/database";

const props = defineProps<{
  showDriverStore?: boolean;
  agentDriverUpdateCount?: number;
}>();

const emit = defineEmits<{
  "toggle-driver-store": [];
  "close-driver-store": [];
}>();

const { t } = useI18n();
const queryStore = useQueryStore();
const settingsStore = useSettingsStore();
const compactTabTitle = computed({
  get: () => settingsStore.editorSettings.compactTabTitle,
  set: (checked: boolean | "indeterminate") => {
    settingsStore.updateEditorSettings({ compactTabTitle: checked === true });
  },
});

function toggleCompactTabTitle() {
  compactTabTitle.value = !compactTabTitle.value;
}

function getTabMenuItems(tab: QueryTab): ContextMenuItem[] {
  return [
    {
      label: t("contextMenu.compactTabTitle"),
      action: toggleCompactTabTitle,
      icon: Check,
      iconClass: compactTabTitle.value ? "opacity-100" : "opacity-0",
    },
    { label: "", separator: true },
    {
      label: tab.pinned ? t("contextMenu.unpin") : t("contextMenu.pin"),
      action: () => queryStore.togglePinnedTab(tab.id),
      icon: Pin,
      iconClass: tab.pinned ? "fill-current" : "",
    },
    { label: "", separator: true },
    { label: t("contextMenu.closeTab"), action: () => queryStore.closeTab(tab.id), icon: X },
    {
      label: t("contextMenu.closeOtherTabs"),
      action: () => queryStore.closeOtherTabs(tab.id),
      disabled: queryStore.tabs.length <= 1,
      icon: X,
    },
    {
      label: t("contextMenu.closeAllTabs"),
      action: () => queryStore.closeAllTabs(),
      variant: "destructive" as const,
      icon: X,
    },
  ];
}

const tabsContainerRef = ref<HTMLElement | null>(null);
const { canScrollLeft, canScrollRight, updateScrollButtons } = useTabScroll(tabsContainerRef);
const tabScrollBehavior = ref<ScrollBehavior>("smooth");

watch(
  () => queryStore.tabs.length,
  () => {
    nextTick(updateScrollButtons);
  },
);

watch(
  () => queryStore.activeTabId,
  () => {
    nextTick(() => {
      const container = tabsContainerRef.value;
      if (!container) return;
      const activeEl = container.querySelector('[data-active-tab="true"]');
      if (activeEl) {
        activeEl.scrollIntoView({ behavior: tabScrollBehavior.value, block: "nearest", inline: "center" });
      }
      updateScrollButtons();
      tabScrollBehavior.value = "smooth";
    });
  },
);

watch(
  () => props.showDriverStore,
  (show) => {
    if (!show) return;
    nextTick(() => {
      const container = tabsContainerRef.value;
      if (!container) return;
      const el = container.querySelector("[data-driver-store-tab]");
      if (el) {
        el.scrollIntoView({ behavior: "smooth", block: "nearest", inline: "center" });
      }
      updateScrollButtons();
    });
  },
);

function tabColorStyle(tab: QueryTab) {
  const color = connectionColor(tab.connectionId);
  const isActive = tab.id === queryStore.activeTabId && !props.showDriverStore;
  const isClassic = settingsStore.editorSettings.appLayout === "classic";
  if (!color) {
    if (isClassic) {
      return isActive ? { boxShadow: "0 1px 0 0 var(--color-background)" } : undefined;
    }
    return isActive
      ? {
          borderColor: "var(--ring)",
        }
      : undefined;
  }

  if (isClassic) {
    return {
      backgroundColor: hexToRgba(color, isActive ? 0.16 : 0.07),
      boxShadow: isActive ? `inset 0 -2px 0 ${color}` : undefined,
    };
  }

  return {
    backgroundColor: hexToRgba(color, isActive ? 0.16 : 0.09),
    borderColor: isActive ? hexToRgba(color, 0.72) : hexToRgba(color, 0.18),
  };
}

function tabIconClass(tab: QueryTab) {
  if (tab.mode === "data" || tab.mode === "objects" || tab.mode === "structure")
    return "text-emerald-600 dark:text-emerald-400";
  return "text-blue-600 dark:text-blue-400";
}

const dataTabs = computed(() => queryStore.tabs.filter((tab) => tab.mode === "data"));
const showPinnedDataTabsMenu = computed(
  () => dataTabs.value.length > 0 && (canScrollLeft.value || canScrollRight.value),
);
const dataTabMenuItems = computed(() =>
  dataTabs.value.map((tab) => ({
    value: tab.id,
    label: tabDisplayTitle(tab, t),
    title: tabDisplayTitle(tab, t),
    icon: Table2,
    iconClass: "text-emerald-600 dark:text-emerald-400",
  })),
);

function activateDataTab(tabId: string) {
  tabScrollBehavior.value = "auto";
  queryStore.activeTabId = tabId;
  emit("close-driver-store");
}

const tabsContainerStyle = computed<CSSProperties>(() => ({
  msOverflowStyle: "none",
  scrollbarWidth: "none",
  WebkitOverflowScrolling: "touch",
  paddingRight: showPinnedDataTabsMenu.value && settingsStore.editorSettings.appLayout !== "classic" ? "28px" : "0px",
}));

const tabTailDragRegionClass = computed(() =>
  showPinnedDataTabsMenu.value ? "w-0 flex-none self-stretch" : "min-w-8 flex-1 self-stretch",
);

const dataTabsMenuContainerClass = computed(() =>
  settingsStore.editorSettings.appLayout === "classic"
    ? "relative z-30 flex shrink-0 items-stretch"
    : "absolute inset-y-0 -right-2 z-30 flex items-stretch",
);
</script>

<template>
  <div
    v-if="queryStore.tabs.length > 0 || showDriverStore"
    class="relative flex border-b shrink-0"
    :class="
      settingsStore.editorSettings.appLayout === 'classic'
        ? 'h-9 items-stretch bg-muted'
        : 'h-10 items-center bg-background px-2'
    "
  >
    <div
      v-if="canScrollLeft"
      class="pointer-events-none absolute left-0 z-10 h-full w-6 bg-linear-to-r from-background from-40% to-transparent"
      aria-hidden="true"
    />
    <div
      ref="tabsContainerRef"
      class="flex-1 flex items-center overflow-x-auto min-w-0"
      :class="settingsStore.editorSettings.appLayout === 'classic' ? '' : 'gap-1.5'"
      :style="tabsContainerStyle"
      @scroll="updateScrollButtons"
    >
      <CustomContextMenu
        v-for="tab in queryStore.tabs"
        :key="tab.id"
        :items="getTabMenuItems(tab)"
        v-slot="{ onContextMenu }"
      >
        <div :class="settingsStore.editorSettings.appLayout === 'classic' ? 'h-full' : ''" @contextmenu="onContextMenu">
          <Tooltip>
            <TooltipTrigger as-child>
              <div
                class="group flex items-center gap-1 px-2 text-xs cursor-pointer transition-colors whitespace-nowrap select-none"
                :class="
                  settingsStore.editorSettings.appLayout === 'classic'
                    ? [
                        compactTabTitle ? 'min-w-24' : 'min-w-38',
                        'h-full border-r border-border/80 dark:border-border/45',
                        tab.id === queryStore.activeTabId && !showDriverStore
                          ? 'bg-background text-foreground font-medium'
                          : 'text-foreground/70 hover:text-foreground/90',
                      ]
                    : [
                        compactTabTitle ? 'min-w-24' : 'min-w-38',
                        'h-7 rounded-md border',
                        tab.id === queryStore.activeTabId && !showDriverStore
                          ? 'text-foreground font-medium'
                          : 'border-border/60 text-foreground/70 hover:border-border hover:text-foreground/90',
                      ]
                "
                :style="tabColorStyle(tab)"
                :data-active-tab="tab.id === queryStore.activeTabId && !showDriverStore"
                @click="
                  queryStore.activeTabId = tab.id;
                  emit('close-driver-store');
                "
                @mousedown.middle.prevent="queryStore.closeTab(tab.id)"
              >
                <span class="shrink-0" :class="tabIconClass(tab)">
                  <Table2 v-if="tab.mode === 'data'" class="h-3.5 w-3.5" />
                  <TableProperties v-else-if="tab.mode === 'objects'" class="h-3.5 w-3.5" />
                  <PencilRuler v-else-if="tab.mode === 'structure'" class="h-3.5 w-3.5" />
                  <Code2 v-else class="h-3.5 w-3.5" />
                </span>
                <span class="min-w-0 truncate flex-1">{{ tabDisplayTitle(tab, t) }}</span>
                <Tooltip>
                  <TooltipTrigger as-child>
                    <button
                      class="inline-flex rounded p-0.5 text-muted-foreground hover:bg-muted-foreground/20 hover:text-foreground focus:opacity-100"
                      :class="tab.pinned ? 'visible text-primary' : 'invisible group-hover:visible'"
                      @click.stop="queryStore.togglePinnedTab(tab.id)"
                    >
                      <Pin class="h-3 w-3" :class="{ 'fill-current': tab.pinned }" />
                    </button>
                  </TooltipTrigger>
                  <TooltipContent>{{ tab.pinned ? t("contextMenu.unpin") : t("contextMenu.pin") }}</TooltipContent>
                </Tooltip>
                <button
                  class="rounded hover:bg-muted-foreground/20 p-0.5 shrink-0"
                  @click.stop="queryStore.closeTab(tab.id)"
                >
                  <X class="h-3 w-3" />
                </button>
              </div>
            </TooltipTrigger>
            <TooltipContent side="bottom" class="text-xs grid grid-cols-[auto_1fr] gap-x-2">
              <template v-for="line in tabTooltipLines(tab, t)" :key="line.label">
                <span class="text-muted-foreground">{{ line.label }}</span>
                <span>{{ line.value }}</span>
              </template>
            </TooltipContent>
          </Tooltip>
        </div>
      </CustomContextMenu>

      <!-- Driver Store Tab -->
      <div
        v-if="showDriverStore"
        data-driver-store-tab
        class="group flex min-w-38 items-center gap-1 px-2 text-xs cursor-pointer transition-colors whitespace-nowrap"
        :class="
          settingsStore.editorSettings.appLayout === 'classic'
            ? ['h-full border-r border-border/80 dark:border-border/45 bg-background text-foreground font-medium']
            : ['h-7 rounded-md border text-foreground font-medium', 'border-ring']
        "
        :style="
          settingsStore.editorSettings.appLayout === 'classic' ? { boxShadow: '0 1px 0 0 var(--color-background)' } : {}
        "
        @click="emit('toggle-driver-store')"
      >
        <span class="shrink-0 text-amber-600 dark:text-amber-400">
          <Package class="h-3.5 w-3.5" />
        </span>
        <span class="min-w-0 truncate flex-1">{{ t("toolbar.driverManager") }}</span>
        <span
          v-if="(agentDriverUpdateCount ?? 0) > 0"
          class="inline-flex h-4 min-w-4 shrink-0 items-center justify-center rounded-full bg-red-500 px-1 text-[10px] font-medium leading-none text-white"
          :aria-label="t('toolbar.updatableDriverCount')"
        >
          {{ (agentDriverUpdateCount ?? 0) > 99 ? "99+" : agentDriverUpdateCount }}
        </span>
        <button class="rounded hover:bg-muted-foreground/20 p-0.5 shrink-0" @click.stop="emit('close-driver-store')">
          <X class="h-3 w-3" />
        </button>
      </div>
      <div :class="tabTailDragRegionClass" data-tauri-drag-region />
    </div>
    <div
      v-if="canScrollRight"
      class="pointer-events-none absolute right-0 z-20 h-full w-6 bg-linear-to-l from-background from-40% to-transparent"
      aria-hidden="true"
    />
    <div v-if="showPinnedDataTabsMenu" :class="dataTabsMenuContainerClass">
      <LightDropdown
        :model-value="queryStore.activeTabId || ''"
        :items="dataTabMenuItems"
        :aria-label="t('tabs.openDataTabs')"
        :trigger-icon="ChevronDown"
        trigger-class="h-full w-7 rounded-none text-foreground/70 hover:text-foreground inline-flex items-center justify-center bg-background"
        trigger-icon-class="h-4 w-4"
        item-icon-class="w-3.5 h-3.5 mr-2"
        item-class="max-w-full"
        content-class="w-auto min-w-36 max-w-60"
        :show-trigger-label="false"
        :show-chevron="false"
        :highlight-selected="false"
        :match-trigger-width="false"
        check-position="none"
        align="end"
        @update:model-value="activateDataTab"
      />
    </div>
  </div>
</template>
