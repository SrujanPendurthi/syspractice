import { ProblemList } from './ProblemList';
import { useRoute } from './useRoute';
import { Workspace } from './Workspace';

/** `/s/<scenarioId>` opens that problem; anything else is the problem list. */
export default function App() {
  const { path, navigate } = useRoute();
  const scenarioId = path.startsWith('/s/') ? decodeURIComponent(path.slice(3)) : null;

  return scenarioId ? (
    <Workspace scenarioId={scenarioId} onBack={() => navigate('/')} />
  ) : (
    <ProblemList onOpen={(id) => navigate(`/s/${encodeURIComponent(id)}`)} />
  );
}
