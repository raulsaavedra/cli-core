type Optionable<T> = {
    option: (...args: unknown[]) => T;
};
export declare function collect(value: string, previous: string[]): string[];
export declare function withJsonOption<T extends Optionable<T>>(command: T, description?: string): T;
export {};
