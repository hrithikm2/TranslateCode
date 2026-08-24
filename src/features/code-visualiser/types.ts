export type TraceEvent = 'line' | 'call' | 'return' | 'exception';

export type SerializedValue =
  | null
  | string
  | number
  | boolean
  | SerializedValue[]
  | { [key: string]: SerializedValue };

export type CallStackFrame = {
  functionName: string;
  line?: number;
  arguments?: Record<string, SerializedValue>;
};

export type ExecutionError = {
  type: string;
  message: string;
  line?: number;
};

export type ExecutionFrame = {
  line: number;
  event: TraceEvent;
  locals: Record<string, SerializedValue>;
  callStack: CallStackFrame[];
  error?: ExecutionError;
};

export type RunnerStatus = 'loading' | 'ready' | 'failed';
export type ExecutionStatus = 'idle' | 'running' | 'complete' | 'error' | 'stopped';
