export function output(data, options = {}) {
    if (options.json) {
        console.log(JSON.stringify(data, null, 2));
        return;
    }
    const formatItem = options.formatItem ?? ((item) => defaultFormatItem(item));
    if (Array.isArray(data)) {
        if (data.length === 0) {
            if (options.emptyMessage) {
                console.log(options.emptyMessage);
            }
            return;
        }
        for (const item of data) {
            console.log(formatItem(item));
        }
        return;
    }
    console.log(formatItem(data));
}
export function error(message) {
    console.error(`Error: ${message}`);
    process.exit(1);
}
export function success(message) {
    console.log(message);
}
function defaultFormatItem(item) {
    if (typeof item === "object" && item !== null) {
        return formatRecord(item);
    }
    return String(item);
}
function formatRecord(record) {
    return Object.entries(record)
        .map(([key, value]) => `${key}: ${String(value)}`)
        .join(", ");
}
