export function isCancelledError(error: unknown): boolean {
  if (error && typeof error === 'object') {
    const record = error as { code?: unknown; message?: unknown };
    if (record.code != null && /cancelled|canceled|OperationCancelled/i.test(String(record.code))) {
      return true;
    }
    if (
      record.message != null &&
      /cancelled|canceled|OperationCancelled|elevationCancelled/i.test(String(record.message))
    ) {
      return true;
    }
  }
  const text =
    error instanceof Error
      ? error.message
      : typeof error === 'string'
        ? error
        : (() => {
            try {
              return JSON.stringify(error);
            } catch {
              return String(error);
            }
          })();
  return /cancelled|canceled|OperationCancelled|elevationCancelled/i.test(text);
}

/** Typed error codes the shell knows how to describe (Rust `CommandErrorCode` + local fallbacks). */
export const APP_ERROR_CODES = [
  'invalidInput',
  'operationBusy',
  'operationCancelled',
  'operationFailed',
  'permissionDenied',
  'taskJoinFailed',
  'notFound',
  'unknown',
] as const;

export type AppErrorCode = (typeof APP_ERROR_CODES)[number];

function isAppErrorCode(value: unknown): value is AppErrorCode {
  return typeof value === 'string' && (APP_ERROR_CODES as readonly string[]).includes(value);
}

/**
 * Map any thrown value to a typed code. Rust commands reject with
 * `{ code, details, retryable }`; free-form messages are never used for UI text.
 */
export function appErrorCode(error: unknown): AppErrorCode {
  if (error && typeof error === 'object' && 'code' in error) {
    const code = (error as { code: unknown }).code;
    if (isAppErrorCode(code)) return code;
  }
  return isCancelledError(error) ? 'operationCancelled' : 'unknown';
}
