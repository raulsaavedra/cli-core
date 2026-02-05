export function collect(value, previous) {
    return previous.concat([value]);
}
export function withJsonOption(command, description = "Output as JSON") {
    return command.option("--json", description);
}
