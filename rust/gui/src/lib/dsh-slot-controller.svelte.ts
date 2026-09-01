import type { CodexPlugin, DshUiSlotContribution } from "./plugin-controller.svelte";

/** Keeps DSH-declared sidebar actions and their safe informational overlay together. */
export class DshSlotController {
  overlay = $state<DshUiSlotContribution | null>(null);

  constructor(private readonly plugins: { codexPlugins: CodexPlugin[] }) {}

  get footerActions(): DshUiSlotContribution[] {
    return this.slots()
      .filter((slot) => slot.slot === "sidebar.footer.action")
      .sort((left, right) => left.order - right.order);
  }

  open = (slot: DshUiSlotContribution): void => {
    const overlay = this.slots().find(
      (candidate) =>
        candidate.slot === "shell.overlay" &&
        candidate.plugin === slot.plugin &&
        candidate.id === slot.id,
    );
    this.overlay = overlay || slot;
  };

  close = (event: MouseEvent): void => {
    if (event.target === event.currentTarget) this.overlay = null;
  };

  closeOverlay = (): void => {
    this.overlay = null;
  };

  /** Drops a view when its declaring plugin slot is no longer available. */
  reconcile = (): void => {
    const overlay = this.overlay;
    if (
      overlay &&
      !this.slots().some(
        (slot) =>
          slot.plugin === overlay.plugin && slot.slot === overlay.slot && slot.id === overlay.id,
      )
    ) {
      this.overlay = null;
    }
  };

  private slots(): DshUiSlotContribution[] {
    return this.plugins.codexPlugins.flatMap((plugin) => plugin.ui_slots || []);
  }
}
