---
name: native-win11-ui
description: Craft authentic, non-AI Windows 11 desktop interfaces in Vanilla HTML/CSS/JS using Fluent/Radix dark surface elevation and inline SVG sprites.
---
# Native Windows 11 Desktop UI Craft
1. Enforce 3-tier dark surface elevation (`#0e1116` canvas, `#13171f` sidebar, `#181d27` surface, `#1f2633` elevated) with `1px solid rgba(255,255,255,0.06)` hairlines.
2. Pair every navigation item, primary button, and card action with a 1.5px stroke `<svg class="icon"><use href="#icon-..."/></svg>` symbol.
3. Use `36px` frameless custom titlebars (`data-tauri-drag-region`) with Windows 11 Minimize, Maximize, and Close controls.
4. Reference `.design-tools/impeccable-main` rules to eliminate generic wireframe grids, neon glow clutter, and all-caps label spam.
