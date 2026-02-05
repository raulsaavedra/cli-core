export type JsonSchema<T> = {
    parse: (input: unknown) => T;
};
export interface JsonInputOptions<T> {
    data?: string;
    file?: string;
    label?: string;
    schema?: JsonSchema<T>;
    allowEmpty?: boolean;
}
export declare function readJsonInput<T = unknown>(options: JsonInputOptions<T>): Promise<T>;
