import {
  chmodSync,
  copyFileSync,
  existsSync,
  mkdirSync,
  readdirSync,
  rmSync,
  statSync,
  symlinkSync,
} from "node:fs";
import { homedir } from "node:os";
import { basename, join, resolve } from "node:path";

export interface InstallOptions {
  SrcDir: string;
  DestDir: string;
  Name?: string;
  Overwrite?: boolean;
  Link?: boolean;
}

export function ResolveSkillsDir(dest = ""): string {
  if (dest !== "") {
    return resolve(dest);
  }

  const home = homedir();
  if (home === "") {
    throw new Error("resolve home dir: home directory is empty");
  }

  return join(home, ".agents", "skills");
}

export function Install(opts: InstallOptions): string {
  if (!opts.SrcDir) {
    throw new Error("source directory is required");
  }
  if (!opts.DestDir) {
    throw new Error("destination directory is required");
  }

  const name = opts.Name || basename(opts.SrcDir);
  const src = resolve(opts.SrcDir);
  const destRoot = resolve(opts.DestDir);
  const dest = join(destRoot, name);

  try {
    mkdirSync(destRoot, { recursive: true, mode: 0o755 });
  } catch (error) {
    throw new Error(`create destination directory: ${errorMessage(error)}`);
  }

  if (existsSync(dest)) {
    if (!opts.Overwrite) {
      throw new Error(`destination exists: ${dest}`);
    }
    try {
      rmSync(dest, { recursive: true, force: true });
    } catch (error) {
      throw new Error(`remove existing destination: ${errorMessage(error)}`);
    }
  }

  if (opts.Link) {
    try {
      symlinkSync(src, dest);
      return dest;
    } catch (error) {
      throw new Error(`create symlink: ${errorMessage(error)}`);
    }
  }

  copyDir(src, dest);
  return dest;
}

function copyDir(src: string, dest: string): void {
  const entries = readSourceDir(src);

  try {
    mkdirSync(dest, { recursive: true, mode: 0o755 });
  } catch (error) {
    throw new Error(`create destination directory: ${errorMessage(error)}`);
  }

  for (const entry of entries) {
    const srcPath = join(src, entry.name);
    const destPath = join(dest, entry.name);
    if (entry.isDirectory()) {
      copyDir(srcPath, destPath);
      continue;
    }

    copyFile(srcPath, destPath);
  }
}

function readSourceDir(path: string) {
  try {
    return readdirSync(path, { withFileTypes: true });
  } catch (error) {
    throw new Error(`read source directory: ${errorMessage(error)}`);
  }
}

function copyFile(src: string, dest: string): void {
  const info = statSourceFile(src);

  try {
    copyFileSync(src, dest);
    chmodSync(dest, info.mode);
  } catch (error) {
    throw new Error(`copy file: ${errorMessage(error)}`);
  }
}

function statSourceFile(path: string) {
  try {
    return statSync(path);
  } catch (error) {
    throw new Error(`stat source file: ${errorMessage(error)}`);
  }
}

function errorMessage(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}
