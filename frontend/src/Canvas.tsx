import { memo, useCallback, useEffect } from 'react';
import {
  addEdge,
  Background,
  Controls,
  Panel,
  Handle,
  Position,
  ReactFlow,
  type Connection,
  type EdgeChange,
  type NodeChange,
  type NodeProps,
  useReactFlow,
} from '@xyflow/react';
import '@xyflow/react/dist/style.css';

import { accentFor, type CanvasEdge, type CanvasNode } from './types';

function ComponentNodeInner({ data, selected }: NodeProps<CanvasNode>) {
  const accent = accentFor(data.kind);
  return (
    <div
      className="rounded-md border border-line bg-raised px-3 py-2 text-sm text-ink shadow-sm"
      style={{
        borderLeft: `3px solid ${accent}`,
        outline: selected ? `2px solid ${accent}` : undefined,
        outlineOffset: 1,
      }}
    >
      <Handle type="target" position={Position.Left} className="!bg-ink-faint" />
      <div className="font-medium">{data.label}</div>
      <div className="font-mono text-xs text-ink-dim">{data.kind}</div>
      <Handle type="source" position={Position.Right} className="!bg-ink-faint" />
    </div>
  );
}

const ComponentNode = memo(ComponentNodeInner);

// Must live outside the component: a fresh object each render makes React Flow
// remount every node on every drag frame.
const nodeTypes = { component: ComponentNode };

/**
 * Re-frames the diagram whenever `fitKey` changes.
 *
 * `fitView` on `<ReactFlow>` only runs on mount, and the canvas mounts empty — so opening a
 * saved attempt drew its nodes wherever they were saved, usually half off-screen, and the
 * first thing anyone did was hunt for the fit button. Keyed rather than keyed on the node
 * count, because adding a first node by hand should not yank the viewport to it.
 */
function FitOnLoad({ fitKey }: { fitKey: number }) {
  const { fitView } = useReactFlow();
  useEffect(() => {
    if (fitKey === 0) return;
    // No need to wait for the new nodes to be measured: React Flow queues a `fitView`
    // issued before then and runs it once they are, so calling it straight away is enough.
    void fitView({ padding: 0.15, maxZoom: 1 });
  }, [fitKey, fitView]);
  return null;
}

interface CanvasProps {
  /** Bump to re-frame the diagram, e.g. after loading a saved attempt. */
  fitKey?: number;
  nodes: CanvasNode[];
  edges: CanvasEdge[];
  onNodesChange: (changes: NodeChange<CanvasNode>[]) => void;
  onEdgesChange: (changes: EdgeChange<CanvasEdge>[]) => void;
  setNodes: (updater: (nodes: CanvasNode[]) => CanvasNode[]) => void;
  setEdges: (updater: (edges: CanvasEdge[]) => CanvasEdge[]) => void;
}

export function Canvas({
  fitKey = 0,
  nodes,
  edges,
  onNodesChange,
  onEdgesChange,
  setNodes,
  setEdges,
}: CanvasProps) {
  const onConnect = useCallback(
    (c: Connection) => setEdges((es) => addEdge({ ...c, animated: true }, es)),
    [setEdges],
  );

  // ponytail: window.prompt for relabeling. Replace with inline editing when someone
  // complains about it, not before.
  const renameNode = useCallback(
    (_: unknown, node: CanvasNode) => {
      const next = window.prompt('Component name', node.data.label);
      if (next === null) return;
      setNodes((ns) =>
        ns.map((n) => (n.id === node.id ? { ...n, data: { ...n.data, label: next } } : n)),
      );
    },
    [setNodes],
  );

  const labelEdge = useCallback(
    (_: unknown, edge: CanvasEdge) => {
      const next = window.prompt('What flows over this edge?', String(edge.label ?? ''));
      if (next === null) return;
      setEdges((es) => es.map((e) => (e.id === edge.id ? { ...e, label: next } : e)));
    },
    [setEdges],
  );

  return (
    <ReactFlow
      nodes={nodes}
      edges={edges}
      nodeTypes={nodeTypes}
      onNodesChange={onNodesChange}
      onEdgesChange={onEdgesChange}
      onConnect={onConnect}
      onNodeDoubleClick={renameNode}
      onEdgeDoubleClick={labelEdge}
      deleteKeyCode={['Backspace', 'Delete']}
      fitView
      // React Flow's default floor is 0.5. The canvas is the column left over after the
      // sidebar and the critique panel, which on a laptop-width window is ~350px — and an
      // eight-node ML diagram needs about 0.3 to fit in that. At 0.5 `fitView` was being
      // clamped and a reopened attempt came back with both ends cut off.
      minZoom={0.2}
      proOptions={{ hideAttribution: false }}
      className="bg-page"
    >
      <Background gap={20} />
      <FitOnLoad fitKey={fitKey} />
      <Controls />
      {/* An empty canvas is the first thing anyone sees on a problem, and it used to say
          nothing at all — the only instructions were in the sidebar, which is not where
          you are looking when you are staring at a blank grid. */}
      {nodes.length === 0 && (
        <Panel position="top-center" className="pointer-events-none mt-24 text-center">
          <p className="text-base text-ink-dim">Nothing drawn yet.</p>
          <p className="mt-1 text-sm text-ink-faint">
            Add a component from the palette, then drag between handles to connect them.
          </p>
        </Panel>
      )}
    </ReactFlow>
  );
}
