import { memo, useCallback } from 'react';
import {
  addEdge,
  Background,
  Controls,
  Handle,
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
      className="rounded-md border border-line bg-white px-3 py-2 text-[12px] text-ink shadow-sm"
      style={{
        borderLeft: `3px solid ${accent}`,
        outline: selected ? `2px solid ${accent}` : undefined,
        outlineOffset: 1,
      }}
    >
      <Handle type="target" position={Position.Left} className="!bg-ink-faint" />
      <div className="font-medium">{data.label}</div>
      <div className="font-mono text-[10px] text-ink-dim">{data.kind}</div>
      <Handle type="source" position={Position.Right} className="!bg-ink-faint" />
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
      className="bg-page"
    >
      <Background color="#e5e7eb" gap={20} />
      <Controls />
    </ReactFlow>
  );
}
