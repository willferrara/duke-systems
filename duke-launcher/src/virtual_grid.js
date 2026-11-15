/**
 * True Virtual-Viewport Grid Recycler for Duke Launcher v2.2.0.
 *
 * Keeps live DOM card nodes capped at ~60 (visible rows + overscan buffer)
 * across both My Games and Search Games (25,000+ catalog titles) and across
 * all cardZoomPx (125px-280px), compact-card, and Couch Mode breakpoints.
 */
export class VirtualCardGrid {
  constructor(options = {}) {
    this.gridEl = options.gridEl || null;
    this.scrollerEl = options.scrollerEl || null;
    this.maxDomNodes = Math.max(12, Number(options.maxDomNodes || 60));
    this.overscanRows = Math.max(1, Number(options.overscanRows ?? 2));
    this.defaultGap = Number(options.defaultGap || 12);
    this.compactGap = Number(options.compactGap || 10);
    this.couchGap = Number(options.couchGap || 20);
    this.couchMinWidth = Number(options.couchMinWidth || 230);
    this.compactMaxMinWidth = Number(options.compactMaxMinWidth || 155);
    this.defaultAspectRatio = Number(options.defaultAspectRatio || 1.6);
    this.defaultBodyHeight = Number(options.defaultBodyHeight || 86);
    this.compactBodyHeight = Number(options.compactBodyHeight || 74);

    this.isGridVisible = options.isGridVisible || (() => true);
    this.getZoomPx = options.getZoomPx || (() => 185);
    this.isCompact = options.isCompact || (() => false);
    this.isCouch = options.isCouch || (() => false);
    this.getItemsCount = options.getItemsCount || (() => 0);
    this.getItem = options.getItem || (() => null);
    this.createCard = options.createCard || (() => document.createElement("article"));
    this.updateCard = options.updateCard || (() => {});
    this.onVisibleRange = options.onVisibleRange || null;

    this._windowEl = null;
    this._mounted = new Map();
    this._pool = [];
    this._dataVersion = 1;
    this._rafId = null;
    this._measurePending = false;
    this._measuredHeights = new Map();
    this._lastLayoutKey = "";
    this._lastRange = { startIndex: -1, endIndex: -1, cols: 0, rowHeight: 0, totalCount: 0 };
    this._emptyMessageActive = false;

    this._onScroll = () => this.scheduleRender();
    this._onResize = () => this.scheduleRender();
    this._resizeObserver = null;

    if (this.scrollerEl) {
      this.scrollerEl.addEventListener("scroll", this._onScroll, { passive: true });
    }
    if (typeof ResizeObserver !== "undefined" && this.gridEl) {
      this._resizeObserver = new ResizeObserver(() => this.scheduleRender());
      this._resizeObserver.observe(this.gridEl);
    }
    window.addEventListener("resize", this._onResize, { passive: true });
  }

  _ensureWindow() {
    if (!this.gridEl) return null;
    if (!this._windowEl || this._windowEl.parentNode !== this.gridEl) {
      if (!this._windowEl) {
        this._windowEl = document.createElement("div");
        this._windowEl.className = "virtual-grid-window";
      }
      this.gridEl.replaceChildren(this._windowEl);
    }
    this.gridEl.classList.add("virtual-grid-host");
    this._emptyMessageActive = false;
    return this._windowEl;
  }

  _clearMountedToPool() {
    for (const cardEl of this._mounted.values()) {
      cardEl.remove();
      if (this._pool.length < this.maxDomNodes) {
        this._pool.push(cardEl);
      }
    }
    this._mounted.clear();
  }

  showEmptyHtml(html) {
    if (!this.gridEl) return;
    this._clearMountedToPool();
    this._emptyMessageActive = true;
    this.gridEl.classList.remove("virtual-grid-host");
    this.gridEl.style.height = "";
    this.gridEl.innerHTML = html;
    this._lastRange = { startIndex: -1, endIndex: -1, cols: 0, rowHeight: 0, totalCount: 0 };
  }

  clear() {
    if (!this.gridEl) return;
    this._clearMountedToPool();
    this._emptyMessageActive = false;
    this.gridEl.classList.add("virtual-grid-host");
    this.gridEl.style.height = "0px";
    if (this._windowEl && this._windowEl.parentNode === this.gridEl) {
      this._windowEl.replaceChildren();
      this._windowEl.style.transform = "translate3d(0, 0, 0)";
    }
    this._lastRange = { startIndex: -1, endIndex: -1, cols: 0, rowHeight: 0, totalCount: 0 };
  }

  scheduleRender() {
    if (this._rafId !== null) return;
    this._rafId = requestAnimationFrame(() => {
      this._rafId = null;
      this.render();
    });
  }

  invalidateAll() {
    this._dataVersion += 1;
    this.render();
  }

  invalidateVisible() {
    this._dataVersion += 1;
    if (!this.isGridVisible() || this._emptyMessageActive) return;
    for (const [idx, cardEl] of this._mounted.entries()) {
      const item = this.getItem(idx);
      this.updateCard(cardEl, item, idx, false);
      cardEl._vgItem = item;
      cardEl._vgVersion = this._dataVersion;
    }
  }

  patchMatched(predicate) {
    if (!this.isGridVisible() || this._emptyMessageActive) return false;
    let patched = 0;
    for (const [idx, cardEl] of this._mounted.entries()) {
      const item = this.getItem(idx);
      if (!item || (predicate && !predicate(item, cardEl, idx))) continue;
      this.updateCard(cardEl, item, idx, false);
      cardEl._vgItem = item;
      cardEl._vgVersion = this._dataVersion;
      patched += 1;
    }
    return patched > 0;
  }

  _computeMetrics(containerWidth) {
    const zoomPx = Math.max(125, Math.min(280, Math.round(Number(this.getZoomPx() || 185))));
    const compact = Boolean(this.isCompact());
    const couch = Boolean(this.isCouch());
    const gap = couch ? this.couchGap : compact ? this.compactGap : this.defaultGap;
    const minWidth = couch
      ? this.couchMinWidth
      : compact
        ? Math.min(this.compactMaxMinWidth, zoomPx)
        : zoomPx;
    const safeWidth = Math.max(240, containerWidth || 900);
    const cols = Math.max(1, Math.floor((safeWidth + gap) / (minWidth + gap)));
    const cardWidth = Math.max(80, (safeWidth - (cols - 1) * gap) / cols);
    const estBody = compact ? this.compactBodyHeight : this.defaultBodyHeight;
    const estHeight = Math.round(cardWidth / this.defaultAspectRatio + estBody);
    const layoutKey = `${Math.round(safeWidth / 16)}:${cols}:${zoomPx}:${compact ? 1 : 0}:${couch ? 1 : 0}`;
    const measured = this._measuredHeights.get(layoutKey);
    const cardHeight =
      measured && Math.abs(measured - estHeight) <= 120 ? measured : estHeight;
    return { cols, gap, zoomPx, compact, couch, layoutKey, cardHeight, rowStride: cardHeight + gap };
  }

  _getGridTopOffsetInScroller() {
    if (!this.gridEl || !this.scrollerEl) return 0;
    const scrollerRect = this.scrollerEl.getBoundingClientRect();
    const gridRect = this.gridEl.getBoundingClientRect();
    return Math.max(0, this.scrollerEl.scrollTop + (gridRect.top - scrollerRect.top));
  }

  render({ resetScroll = false } = {}) {
    if (!this.gridEl || !this.scrollerEl) return;
    if (!this.isGridVisible()) return;

    const totalCount = Math.max(0, Number(this.getItemsCount() || 0));
    if (resetScroll) {
      this.scrollerEl.scrollTop = 0;
    }

    if (totalCount === 0) {
      this.clear();
      return;
    }

    const windowEl = this._ensureWindow();
    if (!windowEl) return;

    const containerWidth = this.gridEl.clientWidth || this.scrollerEl.clientWidth - 40 || 900;
    const metrics = this._computeMetrics(containerWidth);
    const { cols, gap, layoutKey, rowStride } = metrics;

    const totalRows = Math.ceil(totalCount / cols);
    const totalHeight = Math.max(0, totalRows * rowStride - gap);
    this.gridEl.style.height = `${totalHeight}px`;
    windowEl.style.setProperty("--vgrid-cols", String(cols));
    windowEl.style.setProperty("--vgrid-gap", `${gap}px`);

    const viewportHeight = this.scrollerEl.clientHeight || window.innerHeight || 720;
    const scrollTop = this.scrollerEl.scrollTop || 0;
    const gridTop = this._getGridTopOffsetInScroller();
    const relTop = Math.max(0, scrollTop - gridTop);
    const relBottom = Math.max(0, scrollTop + viewportHeight - gridTop);

    const firstVisibleRow = Math.max(0, Math.min(totalRows - 1, Math.floor(relTop / rowStride)));
    const lastVisibleRow = Math.max(
      firstVisibleRow,
      Math.min(totalRows - 1, Math.floor(Math.max(0, relBottom - 1) / rowStride))
    );

    let startRow = Math.max(0, firstVisibleRow - this.overscanRows);
    let endRow = Math.min(totalRows - 1, lastVisibleRow + this.overscanRows);

    while ((endRow - startRow + 1) * cols > this.maxDomNodes && (endRow - startRow) > (lastVisibleRow - firstVisibleRow)) {
      if (endRow - lastVisibleRow >= firstVisibleRow - startRow && endRow > lastVisibleRow) {
        endRow -= 1;
      } else if (startRow < firstVisibleRow) {
        startRow += 1;
      } else {
        break;
      }
    }

    const startIndex = startRow * cols;
    let endIndex = Math.min(totalCount - 1, (endRow + 1) * cols - 1);
    if (endIndex - startIndex + 1 > this.maxDomNodes) {
      endIndex = startIndex + this.maxDomNodes - 1;
    }

    const translateY = startRow * rowStride;
    windowEl.style.transform = `translate3d(0, ${translateY}px, 0)`;

    for (const [idx, cardEl] of Array.from(this._mounted.entries())) {
      if (idx < startIndex || idx > endIndex) {
        cardEl.remove();
        this._mounted.delete(idx);
        if (this._pool.length < this.maxDomNodes) {
          this._pool.push(cardEl);
        }
      }
    }

    for (let idx = startIndex; idx <= endIndex; idx += 1) {
      const item = this.getItem(idx);
      let cardEl = this._mounted.get(idx);
      const isNew = !cardEl;
      if (isNew) {
        cardEl = this._pool.pop() || this.createCard();
        this._mounted.set(idx, cardEl);
      }
      cardEl.style.order = String(idx - startIndex);
      if (isNew || cardEl._vgItem !== item || cardEl._vgVersion !== this._dataVersion || this._lastLayoutKey !== layoutKey) {
        this.updateCard(cardEl, item, idx, isNew);
        cardEl._vgItem = item;
        cardEl._vgVersion = this._dataVersion;
      }
      if (isNew && cardEl.parentNode !== windowEl) {
        windowEl.appendChild(cardEl);
      }
    }

    this._lastLayoutKey = layoutKey;
    this._lastRange = { startIndex, endIndex, cols, rowHeight: metrics.cardHeight, totalCount };

    if (!this._measurePending && !this._measuredHeights.has(layoutKey)) {
      const sample = this._mounted.get(startIndex);
      if (sample) {
        this._measurePending = true;
        requestAnimationFrame(() => {
          this._measurePending = false;
          if (!sample.isConnected) return;
          const h = Math.round(sample.getBoundingClientRect().height);
          if (h >= 120 && h <= 650 && Math.abs(h - metrics.cardHeight) > 2) {
            this._measuredHeights.set(layoutKey, h);
            this.scheduleRender();
          } else if (h >= 120 && h <= 650) {
            this._measuredHeights.set(layoutKey, h);
          }
        });
      }
    }

    if (typeof this.onVisibleRange === "function") {
      this.onVisibleRange(startIndex, endIndex, totalCount);
    }
  }

  scrollToIndex(idx) {
    if (!this.gridEl || !this.scrollerEl) return null;
    const totalCount = Math.max(0, Number(this.getItemsCount() || 0));
    if (totalCount === 0) return null;
    const clamped = Math.max(0, Math.min(totalCount - 1, Number(idx || 0)));
    const containerWidth = this.gridEl.clientWidth || this.scrollerEl.clientWidth - 40 || 900;
    const { cols, rowStride } = this._computeMetrics(containerWidth);
    const row = Math.floor(clamped / cols);
    const gridTop = this._getGridTopOffsetInScroller();
    this.scrollerEl.scrollTop = Math.max(0, gridTop + row * rowStride - 24);
    this.render();
    return this._mounted.get(clamped) || null;
  }

  getStats() {
    return {
      ...this._lastRange,
      mountedCount: this._mounted.size,
      pooledCount: this._pool.length
    };
  }

  destroy() {
    if (this._rafId !== null) {
      cancelAnimationFrame(this._rafId);
      this._rafId = null;
    }
    if (this.scrollerEl) {
      this.scrollerEl.removeEventListener("scroll", this._onScroll);
    }
    window.removeEventListener("resize", this._onResize);
    if (this._resizeObserver) {
      this._resizeObserver.disconnect();
      this._resizeObserver = null;
    }
    this._clearMountedToPool();
    this._pool.length = 0;
  }
}
