import { mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

import { describe, expect, test } from "bun:test";

import { Read } from "./index.ts";

describe("Read", () => {
  test("fails when both data and file are provided", () => {
    expect(() => Read({ Data: "{}", File: "/tmp/data.json" })).toThrow(
      "JSON input: use either --data or --file",
    );
  });

  test("accepts valid JSON from data", () => {
    expect(Read({ Data: '{"hello": "world"}' })).toBe('{"hello": "world"}');
  });

  test("accepts valid JSON from file", () => {
    const dir = mkdtempSync(join(tmpdir(), "cli-core-json-"));
    try {
      const file = join(dir, "payload.json");
      writeFileSync(file, '{\n  "x": 1\n}\n');
      expect(Read({ File: file })).toBe('{\n  "x": 1\n}');
    } finally {
      rmSync(dir, { recursive: true, force: true });
    }
  });

  test("returns null payload when allow empty is enabled", () => {
    expect(Read({ Data: "   ", AllowEmpty: true })).toBe("null");
  });

  test("fails for invalid JSON payloads", () => {
    expect(() => Read({ Data: "{invalid}" })).toThrow(
      "JSON input: invalid JSON",
    );
  });
});
