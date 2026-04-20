import path from "node:path";

export function resolveWorkspace(root) {
  return path.join(root, "workspace");
}

export const settings = {
  retries: 2,
  enabled: true,
};
