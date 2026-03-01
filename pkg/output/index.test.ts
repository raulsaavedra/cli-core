import { describe, expect, test } from "bun:test";

import { Fatalf } from "./index.ts";

describe("Fatalf", () => {
  test("throws an Error with formatted message", () => {
    expect(() => Fatalf("invalid value: %s (%d)", "x", 4)).toThrow(
      "invalid value: x (4)",
    );
  });
});
