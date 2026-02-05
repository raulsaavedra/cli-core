import { existsSync, lstatSync, mkdirSync, readdirSync, readlinkSync, rmSync } from "node:fs";
import { copyFile, mkdir, symlink } from "node:fs/promises";
import { basename, dirname, join, resolve } from "node:path";
import { homedir } from "node:os";
export function resolveSkillsDir(destDir) {
    if (destDir)
        return destDir;
    const codexHome = process.env.CODEX_HOME;
    if (codexHome) {
        return join(codexHome, "skills");
    }
    const fallback = join(homedir(), ".codex", "skills");
    if (existsSync(fallback)) {
        return fallback;
    }
    throw new Error("Skill destination not found. Provide --dest or set CODEX_HOME.");
}
export function ensureSkillSource(srcDir) {
    const skillFile = join(srcDir, "SKILL.md");
    if (!existsSync(skillFile)) {
        throw new Error(`SKILL.md not found in ${srcDir}`);
    }
}
export function resolveSkillSource(options) {
    const skillName = options.skillName;
    const marker = join("skills", skillName, "SKILL.md");
    const walkUp = (startDir) => {
        let dir = resolve(startDir);
        while (true) {
            if (existsSync(join(dir, marker)))
                return join(dir, "skills", skillName);
            const parent = resolve(dir, "..");
            if (parent === dir)
                return null;
            dir = parent;
        }
    };
    const candidates = [];
    candidates.push(options.cwd ?? process.cwd());
    const argv0 = options.argv0 ?? process.argv[0];
    if (argv0 && (argv0.includes("/") || argv0.includes("\\"))) {
        candidates.push(dirname(argv0));
    }
    for (const candidate of candidates) {
        const found = walkUp(candidate);
        if (found)
            return found;
    }
    throw new Error(`Could not locate skill source for '${skillName}' (expected ${marker}). Run from the repo checkout.`);
}
export async function installSkill(options) {
    const mode = options.mode ?? "copy";
    const srcDir = resolve(options.srcDir);
    const name = options.name ?? basename(srcDir);
    const destRoot = resolve(options.destDir);
    const destPath = join(destRoot, name);
    ensureSkillSource(srcDir);
    await mkdir(destRoot, { recursive: true });
    if (existsSync(destPath)) {
        if (!options.overwrite) {
            throw new Error(`Skill destination already exists: ${destPath}`);
        }
        rmSync(destPath, { recursive: true, force: true });
    }
    if (mode === "link") {
        await symlink(srcDir, destPath, "dir");
        return destPath;
    }
    await copyDir(srcDir, destPath);
    return destPath;
}
async function copyDir(srcDir, destDir) {
    await mkdir(destDir, { recursive: true });
    const entries = readdirSync(srcDir, { withFileTypes: true });
    for (const entry of entries) {
        const srcPath = join(srcDir, entry.name);
        const destPath = join(destDir, entry.name);
        if (entry.isDirectory()) {
            await copyDir(srcPath, destPath);
            continue;
        }
        if (entry.isSymbolicLink()) {
            const target = readlinkSync(srcPath);
            await symlink(target, destPath);
            continue;
        }
        if (entry.isFile()) {
            await mkdir(dirname(destPath), { recursive: true });
            await copyFile(srcPath, destPath);
        }
    }
}
