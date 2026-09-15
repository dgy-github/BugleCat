export function pose(x: number, y: number, options?: { tilt?: number; shift?: number }): { rx: number; ry: number; x: number; y: number };
export function mountMotion(element: HTMLElement): {
  preview(x?: number, y?: number): void;
  setEnabled(enabled: boolean): void;
  destroy(): void;
};
