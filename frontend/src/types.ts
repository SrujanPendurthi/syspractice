import type { Edge as RFEdge, Node as RFNode } from '@xyflow/react';

/**
 * Mirror of `backend/src/topology.rs`. These two are hand-synced — if you change a field
 * name or a NodeKind variant here, change it there in the same commit.
 */
export type NodeKind =
  | 'client'
  | 'cdn'
  | 'loadBalancer'
  | 'apiGateway'
  | 'service'
  | 'database'
  | 'cache'
  | 'queue'
  | 'objectStore';

export interface TopologyNode {
  id: string;
  kind: NodeKind;
  label: string;
  x: number;
  y: number;
}

export interface TopologyEdge {
  id: string;
  source: string;
  target: string;
  label?: string;
}

export interface Topology {
  nodes: TopologyNode[];
  edges: TopologyEdge[];
}

export interface Scenario {
  id: string;
  title: string;
  prompt: string;
  functional: string[];
  nonFunctional: string[];
  /** Present on the wire but deliberately never rendered — seeing it defeats the drill. */
  rubric: string[];
}

export interface SessionSummary {
  id: number;
  scenarioId: string;
  nodeCount: number;
  updatedAt: string;
}

/** Palette metadata. Kept next to NodeKind so adding a kind is one edit here plus one in Rust. */
export const NODE_KINDS: { kind: NodeKind; label: string; accent: string }[] = [
  { kind: 'client', label: 'Client', accent: '#94a3b8' },
  { kind: 'cdn', label: 'CDN', accent: '#38bdf8' },
  { kind: 'loadBalancer', label: 'Load Balancer', accent: '#22d3ee' },
  { kind: 'apiGateway', label: 'API Gateway', accent: '#a78bfa' },
  { kind: 'service', label: 'Service', accent: '#4ade80' },
  { kind: 'database', label: 'Database', accent: '#fb923c' },
  { kind: 'cache', label: 'Cache', accent: '#f87171' },
  { kind: 'queue', label: 'Queue', accent: '#facc15' },
  { kind: 'objectStore', label: 'Object Store', accent: '#c084fc' },
];

export const accentFor = (kind: NodeKind): string =>
  NODE_KINDS.find((k) => k.kind === kind)?.accent ?? '#94a3b8';

export type CanvasNodeData = { label: string; kind: NodeKind; [key: string]: unknown };
export type CanvasNode = RFNode<CanvasNodeData>;
export type CanvasEdge = RFEdge;

/** Canvas state -> the serialized topology the backend and the critic consume. */
export function toTopology(nodes: CanvasNode[], edges: CanvasEdge[]): Topology {
  return {
    nodes: nodes.map((n) => ({
      id: n.id,
      kind: n.data.kind,
      label: n.data.label,
      x: n.position.x,
      y: n.position.y,
    })),
    edges: edges.map((e) => ({
      id: e.id,
      source: e.source,
      target: e.target,
      label: typeof e.label === 'string' && e.label ? e.label : undefined,
    })),
  };
}

/** The inverse, for reloading a saved session. */
export function fromTopology(t: Topology): { nodes: CanvasNode[]; edges: CanvasEdge[] } {
  return {
    nodes: t.nodes.map((n) => ({
      id: n.id,
      type: 'component',
      position: { x: n.x, y: n.y },
      data: { label: n.label, kind: n.kind },
    })),
    edges: t.edges.map((e) => ({
      id: e.id,
      source: e.source,
      target: e.target,
      label: e.label,
      animated: true,
    })),
  };
}
