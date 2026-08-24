export const SIDEBAR_DEFAULT_WIDTH = 250;
export const SIDEBAR_MIN_WIDTH = 190;
export const SIDEBAR_MAX_WIDTH = 440;

export class SidebarController {
  open = $state(true);
  width = $state(SIDEBAR_DEFAULT_WIDTH);
  resizing = $state(false);
  private resizeStartX = 0;
  private resizeStartWidth = SIDEBAR_DEFAULT_WIDTH;

  restoreWidth(): void {
    try {
      const savedWidth = Number(localStorage.getItem("ncx.sidebarWidth"));
      if (Number.isFinite(savedWidth)) this.setWidth(savedWidth, false);
    } catch { /* storage is optional */ }
  }

  toggle = (): void => {
    this.open = !this.open;
  };

  setWidth = (width: number, persist = true): void => {
    this.width = this.clampWidth(width);
    if (persist) {
      try { localStorage.setItem("ncx.sidebarWidth", String(this.width)); } catch { /* storage is optional */ }
    }
  };

  beginResize = (event: PointerEvent): void => {
    if (!this.open) return;
    event.preventDefault();
    this.resizing = true;
    this.resizeStartX = event.clientX;
    this.resizeStartWidth = this.width;
    window.addEventListener("pointermove", this.resize);
    window.addEventListener("pointerup", this.stopResize, { once: true });
  };

  handleResizeKey = (event: KeyboardEvent): void => {
    if (event.key === "ArrowLeft" || event.key === "ArrowRight") {
      event.preventDefault();
      this.setWidth(this.width + (event.key === "ArrowRight" ? 16 : -16));
    } else if (event.key === "Home") {
      event.preventDefault();
      this.setWidth(SIDEBAR_MIN_WIDTH);
    } else if (event.key === "End") {
      event.preventDefault();
      this.setWidth(SIDEBAR_MAX_WIDTH);
    }
  };

  private clampWidth(width: number): number {
    const viewportMax = typeof window === "undefined"
      ? SIDEBAR_MAX_WIDTH
      : Math.max(SIDEBAR_MIN_WIDTH, Math.min(SIDEBAR_MAX_WIDTH, Math.floor(window.innerWidth * 0.45)));
    return Math.min(viewportMax, Math.max(SIDEBAR_MIN_WIDTH, Math.round(width)));
  }

  private resize = (event: PointerEvent): void => {
    if (!this.resizing) return;
    this.setWidth(event.clientX - this.resizeStartX + this.resizeStartWidth);
  };

  private stopResize = (): void => {
    if (!this.resizing) return;
    this.resizing = false;
    window.removeEventListener("pointermove", this.resize);
    window.removeEventListener("pointerup", this.stopResize);
  };
}
