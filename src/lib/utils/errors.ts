export function isCancelledError(error: unknown): boolean {
  const text = error instanceof Error ? error.message : String(error);
  return /cancelled|canceled|OperationCancelled/i.test(text);
}
