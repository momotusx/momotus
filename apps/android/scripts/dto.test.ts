import { expect, test } from "bun:test";
import { readFileSync } from "node:fs";
import { generate, outPath, sourceDir } from "./dto";

test("Generated.kt matches the ts-rs types (run `bun run dto`)", () => {
  expect(readFileSync(outPath, "utf8")).toBe(generate(sourceDir));
});

test("tagged unions, enums and tuples come out as expected", () => {
  const kotlin = generate(sourceDir);
  expect(kotlin).toContain('@JsonClassDiscriminator("state")\nsealed interface RowState {');
  expect(kotlin).toContain('@SerialName("failed")\n    data class Failed(');
  expect(kotlin).toContain('@SerialName("loading")\n    data object Loading : RowState');
  expect(kotlin).toContain("enum class Reason {");
  expect(kotlin).toContain("val byReason: List<Tuple2<HideReason, Int>>,");
  expect(kotlin).toContain("val sizeBytes: Long? = null,");
  expect(kotlin).toContain("val progress: Float? = null,");
  expect(kotlin).not.toContain("sealed interface ListDiff");
});

test("the keystore record is a SyncRecord and a class that carries it never prints it", () => {
  const kotlin = generate(sourceDir);
  expect(kotlin).toContain("val record: SyncRecord? = null,");
  expect(kotlin).toContain("val record: SyncRecord,");
  expect(kotlin).toContain('override fun toString() = "Joined(record=hidden)"');
  expect(kotlin).toMatch(/override fun toString\(\) = "SyncReport\(.*record=\$\{record\?\.let \{ "hidden" \}\}/);
  expect(kotlin).not.toContain("Record<string, unknown>");
});
