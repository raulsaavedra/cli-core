export function formatDate(dateStr) {
    const date = new Date(dateStr);
    if (Number.isNaN(date.getTime())) {
        return dateStr;
    }
    return date.toISOString().slice(0, 10);
}
export function formatDateTime(dateStr) {
    const date = new Date(dateStr);
    if (Number.isNaN(date.getTime())) {
        return dateStr;
    }
    return date.toISOString().slice(0, 16).replace("T", " ");
}
export function truncate(text, max) {
    if (text.length <= max)
        return text;
    return text.slice(0, max) + "...";
}
