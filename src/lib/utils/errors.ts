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
