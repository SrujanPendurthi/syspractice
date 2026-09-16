import { memo, useCallback } from 'react';
import {
  addEdge,
  Background,
  Controls,
  Handle,
  MiniMap,
  Position,
  ReactFlow,
  type Connection,
  type EdgeChange,
  type NodeChange,
  type NodeProps,
} from '@xyflow/react';
import '@xyflow/react/dist/style.css';

import { accentFor, type CanvasEdge, type CanvasNode } from './types';

function ComponentNodeInner({ data, selected }: NodeProps<CanvasNode>) {
  const accent = accentFor(data.kind);
  return (
    <div
      className="rounded-md border-2 bg-slate-900 px-3 py-2 text-xs text-slate-100 shadow-lg"
      style={{ borderColor: accent, outline: selected ? `2px solid ${accent}` : undefined }}
    >
      <Handle type="target" position={Position.Left} className="!bg-slate-400" />
      <div className="font-medium">{data.label}</div>
      <div className="text-[10px] uppercase tracking-wide" style={{ color: accent }}>
        {data.kind}
      </div>
      <Handle type="source" position={Position.Right} className="!bg-slate-400" />
    </div>
  );
}

const ComponentNode = memo(ComponentNodeInner);

// Must live outside the component: a fresh object each render makes React Flow
// remount every node on every drag frame.
const nodeTypes = { component: ComponentNode };

interface CanvasProps {
  nodes: CanvasNode[];
  edges: CanvasEdge[];
  onNodesChange: (changes: NodeChange<CanvasNode>[]) => void;
  onEdgesChange: (changes: EdgeChange<CanvasEdge>[]) => void;
  setNodes: (updater: (nodes: CanvasNode[]) => CanvasNode[]) => void;
  setEdges: (updater: (edges: CanvasEdge[]) => CanvasEdge[]) => void;
}

export function Canvas({
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
      proOptions={{ hideAttribution: false }}
      className="bg-slate-950"
    >
      <Background color="#1e293b" gap={20} />
      <Controls className="!bg-slate-800 !text-slate-100" />
      <MiniMap pannable className="!bg-slate-900" nodeColor={(n) => accentFor((n.data as CanvasNode['data']).kind)} />
    </ReactFlow>
  );
}
