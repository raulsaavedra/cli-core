export interface OutputOptions<T = unknown> {
    json?: boolean;
    emptyMessage?: string;
    formatItem?: (item: T) => string;
}
export declare function output<T>(data: T | T[], options?: OutputOptions<T>): void;
export declare function error(message: string): never;
export declare function success(message: string): void;
