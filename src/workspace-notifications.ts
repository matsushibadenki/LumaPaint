import type { DocumentWorkspaceSnapshot, LayerObjectSnapshot } from './bridge';

export interface WorkspaceNotification {
  sequence: number;
  baseSequence: number | null;
  workspace: DocumentWorkspaceSnapshot;
  objectPatches: { layerId: string; upsert: LayerObjectSnapshot[]; order: string[] | null }[];
}
export interface NotificationState { sequence: number; workspace: DocumentWorkspaceSnapshot }
export function applyNotification(previous: NotificationState | undefined, packet: WorkspaceNotification): NotificationState | undefined {
  if (!Number.isSafeInteger(packet.sequence) || packet.sequence < 1) return undefined;
  if (previous && packet.sequence <= previous.sequence) return previous;
  if (packet.baseSequence === null) return { sequence: packet.sequence, workspace: packet.workspace };
  if (!previous || packet.sequence !== packet.baseSequence + 1 || packet.baseSequence !== previous.sequence || packet.workspace.activeId !== previous.workspace.activeId) return undefined;
  const document = packet.workspace.active;
  if (!document) return previous.workspace.active ? undefined : { sequence: packet.sequence, workspace: packet.workspace };
  if (!previous.workspace.active) return undefined;
  const patches = new Map(packet.objectPatches.map(patch => [patch.layerId, patch]));
  if (patches.size !== packet.objectPatches.length || [...patches.keys()].some(id => !document.layers.some(layer => layer.id === id))) return undefined;
  const layers = [];
  for (const layer of document.layers) {
    const old = previous.workspace.active.layers.find(item => item.id === layer.id);
    const patch = patches.get(layer.id);
    if (!old && !patch?.order) return undefined;
    let objects = old?.objects ?? [];
    if (patch) {
      const values = new Map(objects.map(object => [object.id, object]));
      const changed = new Set<string>();
      for (const object of patch.upsert) {
        if (changed.has(object.id)) return undefined;
        changed.add(object.id);
        values.set(object.id, object);
      }
      const order = patch.order ?? objects.map(object => object.id);
      if (new Set(order).size !== order.length || order.some(id => !values.has(id)) || [...changed].some(id => !order.includes(id))) return undefined;
      objects = order.map(id => values.get(id)!);
    }
    layers.push({ ...layer, objects });
  }
  return { sequence: packet.sequence, workspace: { ...packet.workspace, active: { ...document, layers } } };
}
