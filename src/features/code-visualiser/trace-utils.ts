import type { ExecutionFrame, SerializedValue } from './types';

export const POINTER_NAMES = new Set([
  'i', 'j', 'k', 'left', 'right', 'mid', 'middle', 'low', 'high',
  'start', 'end', 'slow', 'fast', 'l', 'r', 'index',
]);

export function isPrimitive(value: SerializedValue): value is null | string | number | boolean {
  return value === null || ['string', 'number', 'boolean'].includes(typeof value);
}

export function isDictionary(value: SerializedValue): value is Record<string, SerializedValue> & { $type: 'dict' } {
  return Boolean(value && typeof value === 'object' && !Array.isArray(value) && value.$type === 'dict');
}

export function isTuple(value: SerializedValue): value is { $type: 'tuple'; items: SerializedValue[] } {
  return Boolean(value && typeof value === 'object' && !Array.isArray(value) && value.$type === 'tuple');
}

export function arrayEntries(frame?: ExecutionFrame) {
  if (!frame) return [];
  return Object.entries(frame.locals).filter((entry): entry is [string, SerializedValue[]] => Array.isArray(entry[1]));
}

export function changedArrayIndexes(
  name: string,
  values: SerializedValue[],
  previousFrame?: ExecutionFrame,
) {
  const previous = previousFrame?.locals[name];
  if (!Array.isArray(previous)) return new Set<number>();
  const changed = new Set<number>();
  values.forEach((value, index) => {
    if (JSON.stringify(value) !== JSON.stringify(previous[index])) changed.add(index);
  });
  return changed;
}

export function pointersForArray(frame: ExecutionFrame | undefined, length: number) {
  if (!frame) return new Map<number, string[]>();
  const pointers = new Map<number, string[]>();
  Object.entries(frame.locals).forEach(([name, value]) => {
    if (!POINTER_NAMES.has(name) || !Number.isInteger(value) || (value as number) < 0 || (value as number) >= length) return;
    const index = value as number;
    pointers.set(index, [...(pointers.get(index) ?? []), name]);
  });
  return pointers;
}

export function formatValue(value: SerializedValue, compact = false): string {
  if (value === null) return 'None';
  if (typeof value === 'string') return JSON.stringify(value);
  if (typeof value === 'boolean') return value ? 'True' : 'False';
  if (typeof value === 'number') return String(value);
  if (Array.isArray(value)) return JSON.stringify(value);
  if (isTuple(value)) return `(${value.items.map((item) => formatValue(item, true)).join(', ')}${value.items.length === 1 ? ',' : ''})`;
  if ('$type' in value && value.$type === 'unsupported') return String(value.value);
  if ('$type' in value && value.$type === 'truncated') return String(value.value);
  const rendered = JSON.stringify(value);
  return compact && rendered.length > 56 ? `${rendered.slice(0, 53)}…` : rendered;
}
