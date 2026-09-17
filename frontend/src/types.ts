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
  difficulty: Difficulty;
  functional: string[];
  nonFunctional: string[];
  // No `rubric` here on purpose: the backend marks it `#[serde(skip)]` so it never
  // reaches the browser at all. Seeing it would defeat the drill.
}

/**
 * Mirror of `Difficulty` in `backend/src/scenarios.rs`. How hard the problem is —
 * unrelated to `Level`, which is the seniority you choose to be interviewed at.
 */
export type Difficulty = 'easy' | 'medium' | 'hard';

/** Mirror of `ScenarioProgress` in `backend/src/db.rs`. Absent means never attempted. */
export interface ScenarioProgress {
  scenarioId: string;
  /** All five stages were walked. Attendance, not quality. */
  solved: boolean;
  attempts: number;
  lastSeen: string;
}

/**
 * Mirror of `Stage` in `backend/src/critic.rs`. The interview runs one stage at a time.
 * Order here is the order they are walked in, and drives the progress indicator.
 */
export type Stage = 'requirements' | 'estimation' | 'highLevel' | 'deepDive' | 'bottlenecks';

export const STAGES: { stage: Stage; label: string }[] = [
  { stage: 'requirements', label: 'Requirements' },
  { stage: 'estimation', label: 'Estimation' },
  { stage: 'highLevel', label: 'High-level design' },
  { stage: 'deepDive', label: 'Deep dive' },
  { stage: 'bottlenecks', label: 'Bottlenecks' },
];

export const stageIndex = (stage: Stage): number =>
  STAGES.findIndex((s) => s.stage === stage);

/** The stage after `stage`, or null at the end of the interview. */
export const nextStage = (stage: Stage): Stage | null =>
  STAGES[stageIndex(stage) + 1]?.stage ?? null;

/** Mirror of `Level` in `backend/src/scenarios.rs`. Selects how much rubric is in scope. */
export type Level = 'junior' | 'senior' | 'staff';

export const LEVELS: { level: Level; label: string }[] = [
  { level: 'junior', label: 'Junior' },
  { level: 'senior', label: 'Senior' },
  { level: 'staff', label: 'Staff' },
];

/**
 * Mirror of `AttemptSummary` in `backend/src/db.rs`. An attempt is one go at a scenario;
 * "session" now means a logged-in browser and nothing else.
 */
export interface AttemptSummary {
  id: number;
  scenarioId: string;
  nodeCount: number;
  updatedAt: string;
}

/** Mirror of `User` in `backend/src/db.rs`. Never carries a password hash. */
export interface User {
  id: number;
  email: string;
  displayName: string;
  /** Moderator. Set by hand in SQL; it only decides whether delete controls are shown. */
  isAdmin: boolean;
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

/** Mirror of `Section` in `backend/src/concepts.rs`. */
export interface ConceptSection {
  heading: string;
  body: string;
}

/** Mirror of `ConceptCard` in `backend/src/routes.rs` — the list shape, no bodies. */
export interface ConceptCard {
  id: string;
  title: string;
  summary: string;
  scenarios: string[];
}

/** Mirror of `Concept` in `backend/src/concepts.rs`. */
export interface Concept extends ConceptCard {
  sections: ConceptSection[];
  probes: string[];
}

/** Mirror of `PostSummary` in `backend/src/db.rs` — the discussion index, no bodies. */
export interface PostSummary {
  id: number;
  scenarioId: string | null;
  title: string;
  author: string;
  authorId: number;
  score: number;
  /** This reader's own vote: -1, 0 or 1. Always 0 signed out. */
  myVote: number;
  comments: number;
  createdAt: string;
}

/** Mirror of `Post` in `backend/src/db.rs`. */
export interface Post extends Omit<PostSummary, 'comments'> {
  body: string;
}

/** Mirror of `Comment` in `backend/src/db.rs`. */
export interface PostComment {
  id: number;
  body: string;
  author: string;
  authorId: number;
  createdAt: string;
}

/**
 * Mirror of `MatchView` in `backend/src/db.rs`.
 *
 * There are no attempt ids here on purpose. Those identify the two boards, and an
 * opponent holding the id of yours mid-race could read your design — the server never
 * sends them, and this type is the reminder not to add them.
 */
export interface Match {
  id: number;
  scenarioId: string;
  level: Level;
  minutes: number;
  hostId: number;
  host: string;
  guestId: number | null;
  guest: string | null;
  startedAt: string | null;
  /** `startedAt + minutes`, computed server-side. Null until somebody joins. */
  deadline: string | null;
  hostSubmitted: boolean;
  guestSubmitted: boolean;
  cancelled: boolean;
  verdict: string | null;
  winner: 'host' | 'guest' | 'draw' | null;
  createdAt: string;
}
