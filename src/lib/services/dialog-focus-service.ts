/**
 * Modal focus handling shared by project-owned dialogs: initial focus, Tab trap, Escape.
 *
 * Dialogs can stack (a confirm opened from a review panel), so only the most recently
 * activated dialog reacts to keys; releasing it hands control back to the one below.
 */

const FOCUSABLE = [
  'button:not([disabled])',
  '[href]',
  'input:not([disabled]):not([type="hidden"])',
  'select:not([disabled])',
  'textarea:not([disabled])',
  '[tabindex]:not([tabindex="-1"])',
].join(',');

interface DialogEntry {
  panel: HTMLElement;
  onEscape: () => void;
}

const stack: DialogEntry[] = [];
let listening = false;

function focusables(panel: HTMLElement): HTMLElement[] {
  return Array.from(panel.querySelectorAll<HTMLElement>(FOCUSABLE)).filter(
    (el) => el.offsetParent !== null || el === document.activeElement
  );
}

function onKeydown(event: KeyboardEvent) {
  const top = stack[stack.length - 1];
  if (!top) return;
  if (event.key === 'Escape') {
    event.preventDefault();
    event.stopPropagation();
    top.onEscape();
    return;
  }
  if (event.key !== 'Tab') return;
  const items = focusables(top.panel);
  if (!items.length) {
    event.preventDefault();
    top.panel.focus();
    return;
  }
  const first = items[0]!;
  const last = items[items.length - 1]!;
  const active = document.activeElement as HTMLElement | null;
  const inside = !!active && top.panel.contains(active);
  if (event.shiftKey && (active === first || !inside)) {
    event.preventDefault();
    last.focus();
  } else if (!event.shiftKey && (active === last || !inside)) {
    event.preventDefault();
    first.focus();
  }
}

/**
 * Activate focus handling for an open dialog panel. Focuses `[data-autofocus]` or the first
 * enabled button, traps Tab inside the panel, and calls `onEscape` on Escape (the caller decides
 * whether dismissing is allowed right now). Returns a release function that restores focus.
 */
export function trapDialogFocus(panel: HTMLElement, onEscape: () => void): () => void {
  const previous = document.activeElement as HTMLElement | null;
  const entry: DialogEntry = { panel, onEscape };
  stack.push(entry);
  if (!listening) {
    document.addEventListener('keydown', onKeydown, true);
    listening = true;
  }
  if (!panel.hasAttribute('tabindex')) panel.setAttribute('tabindex', '-1');
  const initial =
    panel.querySelector<HTMLElement>('[data-autofocus]:not([disabled])') ??
    panel.querySelector<HTMLElement>('button:not([disabled])') ??
    panel;
  initial.focus({ preventScroll: true });

  let released = false;
  return () => {
    if (released) return;
    released = true;
    const index = stack.indexOf(entry);
    if (index >= 0) stack.splice(index, 1);
    if (!stack.length && listening) {
      document.removeEventListener('keydown', onKeydown, true);
      listening = false;
    }
    if (previous && previous.isConnected && typeof previous.focus === 'function') {
      previous.focus({ preventScroll: true });
    }
  };
}
