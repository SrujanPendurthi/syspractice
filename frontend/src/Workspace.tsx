import { useCallback, useEffect, useRef, useState, type ReactNode } from 'react';
import { useEdgesState, useNodesState } from '@xyflow/react';

import { Canvas } from './Canvas';
import { CritiquePanel } from './CritiquePanel';
import { listScenarios, listSessions, loadSession, saveSession, updateSession } from './api';
import {
  fromTopology,
  LEVELS,
  NODE_KINDS,
  toTopology,
  type CanvasEdge,
  type CanvasNode,
  type Level,
  type NodeKind,
  type Scenario,
  type SessionSummary,
  type Stage,
  type Difficulty,
} from './types';

const DIFFICULTY_TEXT: Record<Difficulty, string> = {
  easy: 'text-easy-ink',
  medium: 'text-medium-ink',
  hard: 'text-hard-ink',
};

/** How long the canvas and notes must sit still before the critic starts on its own. */
const IDLE_MS = 8000;

let nodeSeq = 0;
const nextId = () => `n${Date.now().toString(36)}${nodeSeq++}`;

interface WorkspaceProps {
  scenarioId: string;
  onBack: () => void;
}

export function Workspace({ scenarioId, onBack }: WorkspaceProps) {
  const [scenario, setScenario] = useState<Scenario | null>(null);
  const [sessions, setSessions] = useState<SessionSummary[]>([]);
  const [sessionId, setSessionId] = useState<number | null>(null);
  const [status, setStatus] = useState<string | null>(null);
  const [notes, setNotes] = useState('');
  const [stage, setStage] = useState<Stage>('requirements');
  const [level, setLevel] = useState<Level>('senior');

  const [nodes, setNodes, onNodesChange] = useNodesState<CanvasNode>([]);
  const [edges, setEdges, onEdgesChange] = useEdgesState<CanvasEdge>([]);

  // Mirror of the live canvas, so `getTopology` can stay referentially stable and keep
  // the critique panel out of every drag frame's render.
  const graph = useRef({ nodes, edges });
  graph.current = { nodes, edges };
  const getTopology = useCallback(() => toTopology(graph.current.nodes, graph.current.edges), []);

  // Auto-run plumbing. The panel subscribes; App decides when things have gone quiet.
  // A changing `revision` prop would defeat the memo and put the panel back in every
  // drag frame, so the subscription itself has to be the referentially stable thing.
  const idleListeners = useRef(new Set<() => void>());
  const subscribeToIdle = useCallback((cb: () => void) => {
    idleListeners.current.add(cb);
    return () => {
      idleListeners.current.delete(cb);
    };
  }, []);

  // Restarting on every edit is what gives abort-on-change for free: the cleanup clears
  // the pending timer, and the panel aborts its in-flight stream when it re-runs.
  useEffect(() => {
    const t = setTimeout(() => idleListeners.current.forEach((cb) => cb()), IDLE_MS);
    return () => clearTimeout(t);
  }, [nodes, edges, notes]);

  useEffect(() => {
    listScenarios()
      .then((all) => setScenario(all.find((s) => s.id === scenarioId) ?? null))
      .catch((e) => setStatus(`Backend unreachable: ${e.message}`));
    listSessions().then(setSessions).catch(() => undefined);
  }, [scenarioId]);

  const addNode = (kind: NodeKind) =>
    setNodes((ns) => [
      ...ns,
      {
        id: nextId(),
        type: 'component',
        position: { x: 120 + (ns.length % 5) * 180, y: 100 + Math.floor(ns.length / 5) * 120 },
        data: { label: NODE_KINDS.find((k) => k.kind === kind)!.label, kind },
      },
    ]);

  const persist = async (at: Stage) => {
    if (!scenario) return null;
    const topology = getTopology();
    const { id } = sessionId
      ? await updateSession(sessionId, topology, notes, at)
      : await saveSession(scenario.id, topology, notes, at);
    setSessionId(id);
    setSessions(await listSessions());
    return id;
  };

  const save = async () => {
    try {
      const id = await persist(stage);
      if (id !== null) setStatus(`Saved session #${id}`);
    } catch (e) {
      setStatus(e instanceof Error ? e.message : String(e));
    }
  };

  const open = async (id: number) => {
    const s = await loadSession(id);
    const { nodes: n, edges: e } = fromTopology(s.topology);
    setNodes(n);
    setEdges(e);
    setNotes(s.notes);
    setStage(s.stage);
    setSessionId(s.id);
    setStatus(`Opened session #${s.id}`);
  };

  // Progress on the problem list is derived from saved sessions, so advancing has to
  // persist or the list stays empty for anyone who never presses Save.
  const advance = (next: Stage) => {
    setStage(next);
    void persist(next);
  };

  const reset = () => {
    setNodes([]);
    setEdges([]);
    setNotes('');
    setStage('requirements');
    setSessionId(null);
    setStatus(null);
  };

  return (
    <div className="flex h-screen w-screen flex-col bg-page text-ink lg:flex-row">
      <aside className="flex w-full shrink-0 flex-col overflow-y-auto border-b border-line bg-surface lg:w-72 lg:border-r lg:border-b-0">
        <div className="border-b border-line px-4 py-3">
          <button onClick={onBack} className="text-[12px] text-ink-dim hover:text-ink">
            All problems
          </button>
          <h1 className="mt-1.5 flex items-baseline gap-2 text-[15px] font-semibold">
            {scenario?.title ?? 'syspractice'}
            {scenario && (
              <span className={`text-[12px] font-normal capitalize ${DIFFICULTY_TEXT[scenario.difficulty]}`}>
                {scenario.difficulty}
              </span>
            )}
          </h1>
          {status && <p className="mt-1 text-[11px] text-ink-dim">{status}</p>}
        </div>

        <Section title="Scenario">
          <label className="mt-2 flex items-center gap-2 text-[12px] text-ink-dim">
            Interviewing for
            <select
              value={level}
              onChange={(e) => setLevel(e.target.value as Level)}
              className="flex-1 rounded border border-line bg-page px-2 py-1 text-[13px] text-ink"
            >
              {LEVELS.map((l) => (
                <option key={l.level} value={l.level}>
                  {l.label}
                </option>
              ))}
            </select>
          </label>

          {scenario && (
            <div className="mt-3 space-y-3 text-[12px] leading-relaxed text-ink-dim">
              <p className="text-ink">{scenario.prompt}</p>
              <Requirements title="Functional" items={scenario.functional} />
              <Requirements title="Non-functional" items={scenario.nonFunctional} />
            </div>
          )}
        </Section>

        <Section title="Components">
          <div className="grid grid-cols-2 gap-1">
            {NODE_KINDS.map(({ kind, label, accent }) => (
              <button
                key={kind}
                onClick={() => addNode(kind)}
                className="rounded border border-line bg-page px-2 py-1 text-left text-[12px] hover:border-ink-faint"
                style={{ borderLeftColor: accent, borderLeftWidth: 3 }}
              >
                {label}
              </button>
            ))}
          </div>
          <ul className="mt-2 space-y-0.5 text-[11px] text-ink-dim">
            <li>Drag a box to move it</li>
            <li>Drag handle to handle to connect two boxes</li>
            <li>Double-click a box or arrow to rename it</li>
            <li>Select and press Delete to remove</li>
          </ul>
        </Section>

        <Section title="Session">
          <div className="flex gap-1">
            <button onClick={save} className="flex-1 rounded border border-line bg-page px-2 py-1.5 text-[12px] hover:border-ink-faint">
              {sessionId ? `Save #${sessionId}` : 'Save'}
            </button>
            <button onClick={reset} className="rounded border border-line bg-page px-2 py-1.5 text-[12px] hover:border-ink-faint">
              Start over
            </button>
          </div>
          {/* Only this problem's sessions. The list used to carry every scenario's, which
              was harmless when a dropdown set the scenario and actively wrong once the
              route does — opening one would load another problem's diagram under this
              problem's requirements. */}
          <ul className="mt-2 space-y-1">
            {sessions
              .filter((s) => s.scenarioId === scenarioId)
              .map((s) => (
              <li key={s.id}>
                <button
                  onClick={() => open(s.id)}
                  className="w-full rounded px-2 py-1 text-left text-[12px] text-ink-dim hover:bg-line/50"
                >
                  #{s.id} · {s.nodeCount} nodes · {s.updatedAt.slice(0, 10)}
                </button>
              </li>
              ))}
          </ul>
        </Section>
      </aside>

      <main className="min-h-[24rem] flex-1">
        <Canvas
          nodes={nodes}
          edges={edges}
          onNodesChange={onNodesChange}
          onEdgesChange={onEdgesChange}
          setNodes={setNodes}
          setEdges={setEdges}
        />
      </main>

      {scenario && (
        <CritiquePanel
          scenarioId={scenario.id}
          stage={stage}
          level={level}
          onAdvance={advance}
          getTopology={getTopology}
          notes={notes}
          onNotesChange={setNotes}
          subscribeToIdle={subscribeToIdle}
        />
      )}
    </div>
  );
}

function Section({ title, children }: { title: string; children: ReactNode }) {
  return (
    <div className="border-b border-line px-4 py-3">
      <h2 className="mb-2 text-[12px] font-medium text-ink-dim">{title}</h2>
      {children}
    </div>
  );
}

function Requirements({ title, items }: { title: string; items: string[] }) {
  return (
    <div>
      <p className="font-medium text-ink">{title}</p>
      <ul className="mt-1 list-disc space-y-0.5 pl-4 marker:text-ink-faint">
        {items.map((i) => (
          <li key={i}>{i}</li>
        ))}
      </ul>
    </div>
  );
}
