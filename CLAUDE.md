# Waffle (dango)

## Pointer: nothing floats without occluding

gpui hit-tests every hitbox under the pointer, front to back, until one occludes. An element drawn over others (popover, hover card, menu card, floating bar, toast, PiP, overlay button row, anything `absolute()`/`anchored()`/`deferred()` over content) passes hover and clicks through to whatever is behind it unless it blocks them.

- Every floating surface gets `.id(...)` and `.occlude()` on its outermost element, in the same edit that creates it. Use `.block_mouse_except_scroll()` instead only when the content under it must keep scrolling (a header over a list).
- A backdrop that occludes covers the surface only if the surface sits inside it; a card beside its backdrop still needs its own.
- Decoration inside an element that already handles the pointer (a badge, a scrim, a label on a tile) needs nothing. A transparent full-window layer (the GIF layer, drag ghosts) must not occlude.
- Hover-to-open surfaces keep themselves open with their own `on_hover`, since occluding takes the hover from the row under them.
- Check before calling it done: hover and click the surface's edges and its body over something interactive; nothing behind may light up or respond.
