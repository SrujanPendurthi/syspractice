import {
  createContext,
  memo,
  useCallback,
  useContext,
  useEffect,
  useMemo,
  useRef,
  useState,
  type KeyboardEvent,
} from 'react';
import {
  addEdge,
  Background,
  BaseEdge,
  Controls,
  EdgeLabelRenderer,
  getBezierPath,
  Panel,
  Handle,
  Position,
  ReactFlow,
  type Connection,
  type EdgeChange,
  type EdgeProps,
  type NodeChange,
  type NodeProps,
  useReactFlow,
} from '@xyflow/react';
import '@xyflow/react/dist/style.css';

import { accentFor, type CanvasEdge, type CanvasNode } from './types';

/**
 * Which label is being edited, as `node:<id>` or `edge:<id>` — prefixed because node and
 * edge ids come from different generators and nothing stops the two colliding.
 *
 * A context rather than a flag in each node's `data`: `data` is the diagram, and every
 * change to it restarts the idle timer and is autosaved. Opening a text box is not an edit.
 */
interface Editing {
  editing: string | null;
  /** `null` is a cancel. `refocus` hands focus back to the box or arrow, for keyboard users. */
  finish: (key: string, next: string | null, refocus: boolean) => void;
}

const EditContext = createContext<Editing>({ editing: null, finish: () => {} });

/**
 * The one text box both kinds of label use. Enter and blur commit, Escape cancels.
 *
 * `done` is there because Escape unmounts the input, and a focused element being removed
 * can still fire a blur on its way out — which would commit the edit just cancelled.
 */
function LabelInput({ editKey, initial, label }: { editKey: string; initial: string; label: string }) {
  const { finish } = useContext(EditContext);
  const done = useRef(false);
  const end = (next: string | null, refocus: boolean) => {
    if (done.current) return;
    done.current = true;
    finish(editKey, next, refocus);
  };
  return (
    <input
      autoFocus
      defaultValue={initial}
      aria-label={label}
      onFocus={(e) => e.currentTarget.select()}
      onBlur={(e) => end(e.currentTarget.value, false)}
      onKeyDown={(e) => {
        if (e.key === 'Enter') end(e.currentTarget.value, true);
        else if (e.key === 'Escape') end(null, true);
        else return;
        // Kept from React Flow's own key handling: on an edge its Escape blurs the very
        // element focus is being handed back to.
        e.stopPropagation();
      }}
      // Without nodrag/nopan, selecting text with the mouse drags the node or the pane.
      className="nodrag nopan w-full min-w-24 rounded border border-line bg-page px-1 font-medium text-ink"
    />
  );
}

function ComponentNodeInner({ id, data, selected }: NodeProps<CanvasNode>) {
  const accent = accentFor(data.kind);
  const key = `node:${id}`;
  const editing = useContext(EditContext).editing === key;
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
      {editing ? (
        <LabelInput editKey={key} initial={data.label} label="Component name" />
      ) : (
        <div className="font-medium">{data.label}</div>
      )}
      <div className="font-mono text-xs text-ink-dim">{data.kind}</div>
      <Handle type="source" position={Position.Right} className="!bg-ink-faint" />
    </div>
  );
}

const ComponentNode = memo(ComponentNodeInner);

/**
 * React Flow's default bezier, with the label in HTML rather than SVG: an `<input>` cannot
 * live inside an SVG `<text>`, and the label is the thing edited in place.
 */
function LabeledEdgeInner({
  id,
  label,
  sourceX,
  sourceY,
  targetX,
  targetY,
  sourcePosition,
  targetPosition,
  style,
  markerEnd,
  interactionWidth,
}: EdgeProps<CanvasEdge>) {
  const [path, x, y] = getBezierPath({ sourceX, sourceY, targetX, targetY, sourcePosition, targetPosition });
  const key = `edge:${id}`;
  const editing = useContext(EditContext).editing === key;
  const text = typeof label === 'string' ? label : '';
  return (
    <>
      <BaseEdge path={path} style={style} markerEnd={markerEnd} interactionWidth={interactionWidth} />
      {(text || editing) && (
        <EdgeLabelRenderer>
          {/* No double-click handler of its own: a portal still bubbles through the React
              tree, which runs through the edge, so a double-click here already reaches
              `onEdgeDoubleClick` — and a read-only board, which passes none, stays shut. */}
          <div
            className="nodrag nopan absolute rounded bg-page px-1 text-xs text-ink-dim"
            style={{ transform: `translate(-50%, -50%) translate(${x}px, ${y}px)`, pointerEvents: 'all' }}
          >
            {editing ? <LabelInput editKey={key} initial={text} label="What flows over this edge" /> : text}
          </div>
        </EdgeLabelRenderer>
      )}
    </>
  );
}

const LabeledEdge = memo(LabeledEdgeInner);

// Must live outside the component: a fresh object each render makes React Flow
// remount every node on every drag frame. `default` replaces the built-in edge, so edges
// need no `type` and saved topologies load unchanged.
const nodeTypes = { component: ComponentNode };
const edgeTypes = { default: LabeledEdge };

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
  /**
   * A board to look at rather than draw on: no dragging, connecting, deleting or renaming.
   * The handlers below go unused then, which is the only reason they are optional.
   */
  readOnly?: boolean;
  onNodesChange?: (changes: NodeChange<CanvasNode>[]) => void;
  onEdgesChange?: (changes: EdgeChange<CanvasEdge>[]) => void;
  setNodes?: (updater: (nodes: CanvasNode[]) => CanvasNode[]) => void;
  setEdges?: (updater: (edges: CanvasEdge[]) => CanvasEdge[]) => void;
}

export function Canvas({
  fitKey = 0,
  nodes,
  edges,
  readOnly = false,
  onNodesChange,
  onEdgesChange,
  setNodes,
  setEdges,
}: CanvasProps) {
  const flow = useRef<HTMLDivElement>(null);
  const [editing, setEditing] = useState<string | null>(null);

  const onConnect = useCallback(
    (c: Connection) => setEdges?.((es) => addEdge({ ...c, animated: true }, es)),
    [setEdges],
  );

  // A rename goes through `setNodes`/`setEdges`, the same path `window.prompt` used, so it
  // restarts the idle timer and autosaves exactly as any other edit does.
  const finish = useCallback(
    (key: string, next: string | null, refocus: boolean) => {
      setEditing(null);
      const id = key.slice(5);
      const kind = key.startsWith('node:') ? 'node' : 'edge';
      const label = next?.trim();
      // A blank box name reverts: an unnamed box reaches the model as nothing at all. A
      // blank arrow label is a removal, since most arrows need none.
      if (next === null) {
        // Escape: leave it as it was.
      } else if (kind === 'node' && label) {
        setNodes?.((ns) => ns.map((n) => (n.id === id ? { ...n, data: { ...n.data, label } } : n)));
      } else if (kind === 'edge') {
        setEdges?.((es) => es.map((e) => (e.id === id ? { ...e, label: label || undefined } : e)));
      }
      const sel = `.react-flow__${kind}[data-id="${CSS.escape(id)}"]`;
      if (refocus) flow.current?.querySelector<HTMLElement>(sel)?.focus();
    },
    [setNodes, setEdges],
  );

  const edit = useMemo(() => ({ editing, finish }), [editing, finish]);
  const editNode = useCallback((_: unknown, n: CanvasNode) => setEditing(`node:${n.id}`), []);
  const editEdge = useCallback((_: unknown, e: CanvasEdge) => setEditing(`edge:${e.id}`), []);

  // The keyboard way in: Tab to a box or an arrow and press Enter. React Flow selects it on
  // the same key, which does no harm.
  const onKeyDown = (e: KeyboardEvent) => {
    if (e.key !== 'Enter' || editing) return;
    const el = (e.target as Element).closest('.react-flow__node, .react-flow__edge');
    const id = el?.getAttribute('data-id');
    if (el && id) setEditing(`${el.classList.contains('react-flow__node') ? 'node' : 'edge'}:${id}`);
  };

  return (
    <EditContext.Provider value={edit}>
      <ReactFlow
        ref={flow}
        nodes={nodes}
        edges={edges}
        nodeTypes={nodeTypes}
        edgeTypes={edgeTypes}
        onNodesChange={onNodesChange}
        onEdgesChange={onEdgesChange}
        onConnect={readOnly ? undefined : onConnect}
        onNodeDoubleClick={readOnly ? undefined : editNode}
        onEdgeDoubleClick={readOnly ? undefined : editEdge}
        onKeyDown={readOnly ? undefined : onKeyDown}
        nodesDraggable={!readOnly}
        nodesConnectable={!readOnly}
        elementsSelectable={!readOnly}
        deleteKeyCode={readOnly ? null : ['Backspace', 'Delete']}
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
        <Controls showInteractive={!readOnly} />
        {/* An empty canvas is the first thing anyone sees on a problem, and it used to say
            nothing at all — the only instructions were in the sidebar, which is not where
            you are looking when you are staring at a blank grid. */}
        {nodes.length === 0 && !readOnly && (
          <Panel position="top-center" className="pointer-events-none mt-24 text-center">
            <p className="text-base text-ink-dim">Nothing drawn yet.</p>
            <p className="mt-1 text-sm text-ink-faint">
              Add a component from the palette, then drag between handles to connect them.
            </p>
          </Panel>
        )}
      </ReactFlow>
    </EditContext.Provider>
  );
}
