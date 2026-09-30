import { expect, test } from "bun:test";
import { latestCompatibilityDate } from "../../src/lib/compatibility";

test("compatibility date is bounded by both the binary and UTC day", () => {
  expect(latestCompatibilityDate("2026-09-25", "2026-09-30")).toBe(
    "2026-09-25",
  );
  expect(latestCompatibilityDate("2026-10-02", "2026-09-30")).toBe(
    "2026-09-30",
  );
});
