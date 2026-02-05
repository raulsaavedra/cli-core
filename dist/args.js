export function hasValue(value) {
    return value !== undefined && value !== null;
}
export function requireAtLeastOne(values, message) {
    const ok = Object.values(values).some((value) => hasValue(value));
    if (!ok) {
        throw new Error(message);
    }
}
export function requireExactlyOne(values, message) {
    const count = Object.values(values).filter((value) => hasValue(value)).length;
    if (count !== 1) {
        throw new Error(message);
    }
}
export function requireAll(values, message) {
    const ok = Object.values(values).every((value) => hasValue(value));
    if (!ok) {
        throw new Error(message);
    }
}
export function parseIntOption(value, label) {
    const parsed = typeof value === "number" ? value : Number.parseInt(String(value), 10);
    if (Number.isNaN(parsed)) {
        throw new Error(`Invalid ${label}`);
    }
    return parsed;
}
