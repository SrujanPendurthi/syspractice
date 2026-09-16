import { useCallback, useEffect, useRef, useState, type ReactNode } from 'react';
import { useEdgesState, useNodesState } from '@xyflow/react';

import { Canvas } from './Canvas';
import { CritiquePanel } from './CritiquePanel';
import { listScenarios, listSessions, loadSession, saveSession, updateSession } from './api';
import {
  fromTopology,
  NODE_KINDS,
  toTopology,
  type CanvasEdge,
  type CanvasNode,
  type NodeKind,
  type Scenario,
  type SessionSummary,
} from './types';

let nodeSeq = 0;
const nextId = () => `n${Date.now().toString(36)}${nodeSeq++}`;

export default function App() {
  const [scenarios, setScenarios] = useState<Scenario[]>([]);
  const [scenario, setScenario] = useState<Scenario | null>(null);
  const [sessions, setSessions] = useState<SessionSummary[]>([]);
  const [sessionId, setSessionId] = useState<number | null>(null);
  const [status, setStatus] = useState<string | null>(null);

  const [nodes, setNodes, onNodesChange] = useNodesState<CanvasNode>([]);
  const [edges, setEdges, onEdgesChange] = useEdgesState<CanvasEdge>([]);

  // Mirror of the live canvas, so `getTopology` can stay referentially stable and keep
  // the critique panel out of every drag frame's render.
  const graph = useRef({ nodes, edges });
  graph.current = { nodes, edges };
  const getTopology = useCallback(() => toTopology(graph.current.nodes, graph.current.edges), []);

  useEffect(() => {
    listScenarios()
      .then((s) => {
        setScenarios(s);
        setScenario((cur) => cur ?? s[0] ?? null);
      })
      .catch((e) => setStatus(`Backend unreachable: ${e.message}`));
    listSessions().then(setSessions).catch(() => undefined);
  }, []);

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

  const save = async () => {
    if (!scenario) return;
    try {
      const topology = getTopology();
      const { id } = sessionId
        ? await updateSession(sessionId, topology)
        : await saveSession(scenario.id, topology);
      setSessionId(id);
      setStatus(`Saved session #${id}`);
      setSessions(await listSessions());
    } catch (e) {
      setStatus(e instanceof Error ? e.message : String(e));
    }
  };

  const open = async (id: number) => {
    const s = await loadSession(id);
    const { nodes: n, edges: e } = fromTopology(s.topology);
    setNodes(n);
    setEdges(e);
    setSessionId(s.id);
    setScenario(scenarios.find((x) => x.id === s.scenarioId) ?? null);
    setStatus(`Opened session #${s.id}`);
  };

  const reset = () => {
    setNodes([]);
    setEdges([]);
    setSessionId(null);
    setStatus(null);
  };

  return (
    <div className="flex h-screen w-screen bg-slate-950 text-slate-100">
      <aside className="flex w-72 shrink-0 flex-col overflow-y-auto border-r border-slate-800 bg-slate-900">
        <div className="border-b border-slate-800 px-4 py-3">
          <h1 className="text-sm font-semibold">syspractice</h1>
          {status && <p className="mt-1 text-[11px] text-slate-400">{status}</p>}
        </div>

        <Section title="Scenario">
          <select
            value={scenario?.id ?? ''}
            onChange={(e) => {
              setScenario(scenarios.find((s) => s.id === e.target.value) ?? null);
              reset();
            }}
            className="w-full rounded border border-slate-700 bg-slate-800 px-2 py-1 text-xs"
          >
            {scenarios.map((s) => (
              <option key={s.id} value={s.id}>
                {s.title}
              </option>
            ))}
          </select>

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
          <ul className="mt-2 space-y-1">
            {sessions.map((s) => (
              <li key={s.id}>
                <button
                  onClick={() => open(s.id)}
                  className="w-full rounded px-2 py-1 text-left text-[11px] text-slate-400 hover:bg-slate-800"
                >
                  #{s.id} · {s.scenarioId} · {s.nodeCount} nodes
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

      {scenario && <CritiquePanel scenarioId={scenario.id} getTopology={getTopology} />}
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
