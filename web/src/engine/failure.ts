// Local API v1 errors are data, not instructions to retry a mutation. Older
// engines may supply only `error`; unsupported versions keep a safe fallback.
export interface EngineFailure {
  message: string;
  code?: string;
  recovery?: string;
  recoveryHint?: string;
}

export function readEngineFailure(value: unknown): EngineFailure {
  const fallback = 'The local engine could not complete this request.';
  if (!value || typeof value !== 'object' || Array.isArray(value)) return { message: fallback };
  const record = value as Record<string, unknown>;
  if (record.schema_version !== undefined && record.schema_version !== 1)
    return {
      message: 'The engine and this app use different API versions. Update the app and reconnect.',
      code: 'unsupported_version',
    };
  return {
    message: typeof record.error === 'string' && record.error ? record.error : fallback,
    ...(record.schema_version === 1
      ? {
          code: typeof record.code === 'string' ? record.code : undefined,
          recovery: typeof record.recovery === 'string' ? record.recovery : undefined,
          recoveryHint: typeof record.recovery_hint === 'string' ? record.recovery_hint : undefined,
        }
      : {}),
  };
}

export class EngineRequestError extends Error {
  constructor(
    message: string,
    public status: number,
    public code?: string,
    public recovery?: string,
    public recoveryHint?: string,
  ) {
    super(message);
  }
}

export type ConnectionProblem =
  | 'missing'
  | 'authorization'
  | 'unreachable'
  | 'timeout'
  | 'incompatible';

export class EngineConnectionError extends Error {
  constructor(public reason: Exclude<ConnectionProblem, 'authorization'>) {
    super(
      {
        missing: 'Open the connection link printed by the local engine.',
        unreachable: 'The local engine is unavailable. Start it and reconnect.',
        timeout: 'The engine did not answer in time. Check that it is running, then retry.',
        incompatible:
          'The engine and this app use different API versions. Update the app and reconnect.',
      }[reason],
    );
  }
}

export function connectionProblem(error: unknown): ConnectionProblem | null {
  if (error instanceof EngineConnectionError) return error.reason;
  if (error instanceof EngineRequestError) {
    if (error.code === 'unsupported_version') return 'incompatible';
    if (error.status === 401) return 'authorization';
  }
  return null;
}
