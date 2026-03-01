import {
  lstatSync,
  mkdirSync,
  mkdtempSync,
  readFileSync,
  rmSync,
  writeFileSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";

import { describe, expect, test } from "bun:test";

import { Install, ResolveSkillsDir } from "./index.ts";

describe("ResolveSkillsDir", () => {
  test("returns absolute destination when provided", () => {
    expect(ResolveSkillsDir("./skills")).toBe(resolve("./skills"));
  });
});

describe("Install", () => {
  test("copies directory content to destination", () => {
    const dir = mkdtempSync(join(tmpdir(), "cli-core-skills-"));
    try {
      const src = join(dir, "src");
      const dest = join(dir, "dest");
      mkdirSync(join(src, "references"), { recursive: true });
      writeFileSync(join(src, "SKILL.md"), "# Skill");
      writeFileSync(join(src, "references", "notes.md"), "hello");

      const installed = Install({
        SrcDir: src,
        DestDir: dest,
        Name: "installed-skill",
      });

      expect(installed).toBe(join(dest, "installed-skill"));
      expect(readFileSync(join(installed, "SKILL.md"), "utf8")).toBe("# Skill");
      expect(
        readFileSync(join(installed, "references", "notes.md"), "utf8"),
      ).toBe("hello");
    } finally {
      rmSync(dir, { recursive: true, force: true });
    }
  });

  test("supports symlink mode", () => {
    const dir = mkdtempSync(join(tmpdir(), "cli-core-skills-link-"));
    try {
      const src = join(dir, "src");
      const dest = join(dir, "dest");
      mkdirSync(src, { recursive: true });
      writeFileSync(join(src, "SKILL.md"), "# Skill");

      const installed = Install({
        SrcDir: src,
        DestDir: dest,
        Link: true,
      });

      const linkedTarget = lstatSync(installed).isSymbolicLink();
      expect(linkedTarget).toBe(true);
    } finally {
      rmSync(dir, { recursive: true, force: true });
    }
  });

  test("fails when destination exists and overwrite is disabled", () => {
    const dir = mkdtempSync(join(tmpdir(), "cli-core-skills-overwrite-"));
    try {
      const src = join(dir, "src");
      const dest = join(dir, "dest");
      mkdirSync(src, { recursive: true });
      mkdirSync(join(dest, "src"), { recursive: true });
      writeFileSync(join(src, "SKILL.md"), "# Skill");
      writeFileSync(join(dest, "src", "SKILL.md"), "# Existing");

      expect(() =>
        Install({
          SrcDir: src,
          DestDir: dest,
        }),
      ).toThrow("destination exists");
    } finally {
      rmSync(dir, { recursive: true, force: true });
    }
  });
});
