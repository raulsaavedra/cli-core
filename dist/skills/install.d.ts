export type SkillInstallMode = "copy" | "link";
export interface SkillInstallOptions {
    srcDir: string;
    destDir: string;
    name?: string;
    mode?: SkillInstallMode;
    overwrite?: boolean;
}
export interface ResolveSkillSourceOptions {
    /**
     * The skill directory name under `skills/` (e.g. `quiz`, `tickets`).
     * The resulting source dir will be `<repoRoot>/skills/<skillName>`.
     */
    skillName: string;
    /**
     * Defaults to `process.cwd()`.
     */
    cwd?: string;
    /**
     * Defaults to `process.argv[0]` if available. For compiled binaries this is
     * usually the best anchor.
     */
    argv0?: string;
}
export declare function resolveSkillsDir(destDir?: string): string;
export declare function ensureSkillSource(srcDir: string): void;
export declare function resolveSkillSource(options: ResolveSkillSourceOptions): string;
export declare function installSkill(options: SkillInstallOptions): Promise<string>;
