import { readFileSync } from "node:fs";
import { strict as assert } from "node:assert";
import test from "node:test";

const source = readFileSync("apps/desktop/src/components/grid/DataGrid.vue", "utf8");

test("data grid uses lazy transpose rows and Tab keyboard toggle", () => {
  assert.match(source, /buildVisibleTransposeRows/);
  assert.match(source, /nextKeyboardTransposeState/);
  assert.match(source, /isToggleTransposeShortcut/);
  assert.match(source, /settingsStore\.editorSettings\.shortcuts/);
});

test("transpose cells reuse inline editing controls", () => {
  assert.match(source, /canEditCellItem\(displayItems\[cell\.recordIndex\], cell\.valueIndex\)/);
  assert.match(source, /startEdit\(displayItems\[cell\.recordIndex\]\.id, cell\.valueIndex\)/);
  assert.match(source, /editingCell\?\.rowId === displayItems\[cell\.recordIndex\]\?\.id/);
});

test("transpose mode follows appended rows and survives rollback refresh", () => {
  assert.match(source, /addRow:\s*addEditorRow/);
  assert.match(source, /function addRow\(\)[\s\S]*?focusAppendedTransposeRecord\(\)/);
  assert.match(source, /preserveTransposeOnNextResult/);
  assert.match(
    source,
    /function onToolbarRollback\(\)[\s\S]*?preserveTransposeOnNextResult\.value = showTranspose\.value/,
  );
  assert.match(source, /nextTransposeStateForRecordCount/);
});
