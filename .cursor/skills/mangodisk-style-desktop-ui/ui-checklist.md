# UI checklist (MangoDisk-like)

Copy and tick while implementing a page.

## Shell

- [ ] Custom titlebar / drag regions for Windows (and macOS if needed)
- [ ] Collapsible sidebar with icon-only + expanded labels
- [ ] Grouped nav; active state with left primary indicator
- [ ] Busy indicator on pages with running ops
- [ ] Page shell: title, subtitle, header actions outside scroll body
- [ ] Content mode chosen: `document` or `workspace`
- [ ] Min window size respected; no layout jump on nav

## Visual

- [ ] Semantic CSS tokens only (no one-off hex in feature CSS)
- [ ] Light + dark themes both readable
- [ ] Dense typography (row ~13px primary, meta smaller)
- [ ] Radius ~8px; soft surfaces; workspace may be transparent on dark
- [ ] Icons via components, not inline SVG
- [ ] Brand primary ≠ copied competitor orange unless intentional

## Interaction

- [ ] Buttons do not translate/scale on hover/active
- [ ] Destructive action requires confirm dialog
- [ ] Selection action bar appears only with selection
- [ ] Empty state has one clear CTA
- [ ] Progress overlay for long filesystem work
- [ ] Cancel path exists and is safe to spam-click
- [ ] Window shown only after Vue mount (no white flash)
- [ ] No scan/heavy I/O on cold start before first paint
- [ ] Secondary pages lazy-loaded (primary page eager)

## Data / backend

- [ ] Page talks to a store, store talks to a service
- [ ] Progress listener registered before invoke
- [ ] Listener always cleaned up
- [ ] Typed codes drive badges/status UI
- [ ] User strings in locale files
- [ ] No auto-delete; scan is read-only by default

## Accessibility / desktop polish

- [ ] Focus-visible rings present
- [ ] Tooltips when sidebar collapsed
- [ ] Text selection disabled except inputs
- [ ] Scrollbars consistent with global utilities
- [ ] Reduced-motion: UI calm; ops indicators still alive but slower
