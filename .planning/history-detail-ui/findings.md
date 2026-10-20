# Findings

- The preview effect depends on item object identity and clears state before checking asynchronous cache.
- Delivery hook clears its snapshot on entry change and has no cache.
- Image dimensions card is conditionally inserted after load.
- All three PreviewFrame skins have overflow hidden and flex-shrink default, while ordinary TextPreview has no scroll container.
- Prior fixes for removing device summary and resetting detail scrolling are already committed; initial working tree is clean.

- Reproduction confirmed all three failures on each skin, not Linux-only.
- Preview cache now exposes a synchronous fresh read; preview state is keyed by entry identity instead of the item object reference.
- Delivery snapshots are bounded to 32 recent entries within the mounted hook, and always revalidated on selection or matching WS events.
- PreviewFrame owns a bounded responsive viewport (10rem–28rem at 40vh) and scrollable body on all skins; it must not flex-shrink and clip its content.
