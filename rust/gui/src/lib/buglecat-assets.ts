export type BugleCatAssetSize = 24 | 32 | 48 | 64;

export const buglecatAsset = (name: string, size: BugleCatAssetSize = 32): string =>
  `/assets/buglecat/${size}/${name}.png`;
