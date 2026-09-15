import { mountMotion } from "./motion.js";

/** Bind navigation controls with stable hit boxes; only decorative children move. */
export function hoverDepth(root: HTMLElement, selector: string) {
  const cards = new Map<HTMLElement, ReturnType<typeof mountMotion>>();
  const sync = () => {
    for (const [node, motion] of cards) {
      if (!root.contains(node)) { motion.destroy(); cards.delete(node); }
    }
    for (const node of root.querySelectorAll<HTMLElement>(selector)) {
      if (cards.has(node)) continue;
      node.classList.add("hover-depth");
      node.dataset.tilt = "4";
      node.dataset.shift = "3";
      cards.set(node, mountMotion(node));
    }
  };
  sync();
  const observer = new MutationObserver(sync);
  observer.observe(root, { childList: true, subtree: true });
  return { destroy() {
    observer.disconnect();
    for (const [node, motion] of cards) { motion.destroy(); node.classList.remove("hover-depth"); }
    cards.clear();
  } };
}
