# cli-core

Shared utilities for Raul’s CLI tools (Bun + TypeScript).

## Install (local workspace)
```bash
bun add file:../packages/cli-core
```

## Modules

### `args`
- `requireAtLeastOne(values, message)`
- `requireExactlyOne(values, message)`
- `requireAll(values, message)`
- `parseIntOption(value, label)`

### `commander`
- `collect(value, previous)`
- `withJsonOption(command, description?)`

### `format`
- `formatDate(dateStr)`
- `formatDateTime(dateStr)`
- `truncate(text, max)`

### `json-input`
- `readJsonInput({ data, file, label?, schema?, allowEmpty? })`

### `output`
- `output(data, { json?, emptyMessage?, formatItem? })`
- `error(message)` (prints + exits)
- `success(message)`

### `sqlite`
- `dbPath(appName, filename)`
- `ensureDirForFile(path)`
- `applyPragmas(db, pragmas)`
- `openSqliteDb({ appName, filename, path?, migrate?, pragmas? })`

### `stdio`
- `readStdin()`

### `skills/install`
- `resolveSkillsDir(dest?)`
- `resolveSkillSource({ skillName, cwd?, argv0? })`
- `installSkill({ srcDir, destDir, name?, mode?, overwrite? })`

## Examples

```ts
import { openSqliteDb, output, readJsonInput } from "@raulsaavedra/cli-core";

const db = openSqliteDb({
  appName: "tickets",
  filename: "tickets.db",
  pragmas: ["foreign_keys = ON"],
  migrate,
});

const payload = await readJsonInput({ data: opts.data, file: opts.file, label: "Instructions" });
output(payload, { json: true });
```

```ts
import { installSkill, resolveSkillsDir } from "@raulsaavedra/cli-core";

const destDir = resolveSkillsDir(process.env.SKILL_DEST);
await installSkill({
  srcDir: new URL("../skills/tickets", import.meta.url).pathname,
  destDir,
  name: "tickets",
  overwrite: true,
});
```
