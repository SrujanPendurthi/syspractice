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
} from './types';

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
    <div className="flex h-screen w-screen bg-slate-950 text-slate-100">
      <aside className="flex w-72 shrink-0 flex-col overflow-y-auto border-r border-slate-800 bg-slate-900">
        <div className="border-b border-slate-800 px-4 py-3">
          <button
            onClick={onBack}
            className="text-[11px] text-slate-400 hover:text-slate-200"
          >
            ← All problems
          </button>
          <h1 className="mt-1 text-sm font-semibold">{scenario?.title ?? 'syspractice'}</h1>
          {status && <p className="mt-1 text-[11px] text-slate-400">{status}</p>}
        </div>

        <Section title="Scenario">
          <label className="mt-2 flex items-center gap-2 text-[11px] text-slate-400">
            Interviewing for
            <select
              value={level}
              onChange={(e) => setLevel(e.target.value as Level)}
              className="flex-1 rounded border border-slate-700 bg-slate-800 px-2 py-1 text-xs text-slate-100"
            >
              {LEVELS.map((l) => (
                <option key={l.level} value={l.level}>
                  {l.label}
                </option>
              ))}
            </select>
          </label>

          {scenario && (
            <div className="mt-3 space-y-3 text-[11px] leading-relaxed text-slate-400">
              <p className="text-slate-300">{scenario.prompt}</p>
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
                className="rounded border border-slate-700 px-2 py-1 text-left text-[11px] hover:bg-slate-800"
                style={{ borderLeftColor: accent, borderLeftWidth: 3 }}
              >
                {label}
              </button>
            ))}
          </div>
          <p className="mt-2 text-[10px] text-slate-500">
            Drag to arrange · drag handle to handle to connect · double-click to rename ·
            Delete to remove
          </p>
        </Section>

        <Section title="Session">
          <div className="flex gap-1">
            <button onClick={save} className="flex-1 rounded bg-slate-700 px-2 py-1 text-xs hover:bg-slate-600">
              {sessionId ? `Save #${sessionId}` : 'Save'}
            </button>
            <button onClick={reset} className="rounded bg-slate-800 px-2 py-1 text-xs hover:bg-slate-700">
              New
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
                  className="w-full rounded px-2 py-1 text-left text-[11px] text-slate-400 hover:bg-slate-800"
                >
                  #{s.id} · {s.nodeCount} nodes · {s.updatedAt.slice(0, 10)}
                </button>
              </li>
              ))}
          </ul>
        </Section>
      </aside>

      <main className="flex-1">
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
    <div className="border-b border-slate-800 px-4 py-3">
      <h2 className="mb-2 text-[11px] font-semibold uppercase tracking-wide text-slate-500">
        {title}
      </h2>
      {children}
    </div>
  );
}

function Requirements({ title, items }: { title: string; items: string[] }) {
  return (
    <div>
      <p className="font-medium text-slate-300">{title}</p>
      <ul className="mt-1 list-disc space-y-0.5 pl-4">
        {items.map((i) => (
          <li key={i}>{i}</li>
        ))}
      </ul>
    </div>
  );
}
