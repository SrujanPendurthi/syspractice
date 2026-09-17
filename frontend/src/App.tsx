import { AuthPage } from './AuthPage';
import { Discuss } from './Discuss';
import { Learn } from './Learn';
import { ProblemList } from './ProblemList';
import { useRoute } from './useRoute';
import { AuthProvider } from './useAuth';
import { Workspace } from './Workspace';

/**
 * `/s/<scenarioId>` opens that problem, `/learn` is the concept index and
 * `/learn/<conceptId>` one article, `/discuss` and `/discuss/<postId>` are the forum,
 * `/login` is the account form, anything else is the problem list. There is deliberately no redirect wall: browsing problems, reading
 * concepts and drawing on the canvas all work signed out, and only saving, critiquing and
 * tracking what you have read need an account.
 */
function Routes() {
  const { path, navigate } = useRoute();
  const scenarioId = path.startsWith('/s/') ? decodeURIComponent(path.slice(3)) : null;
  const openScenario = (id: string) => navigate(`/s/${encodeURIComponent(id)}`);

  if (path === '/login') {
    return <AuthPage onDone={() => navigate('/')} onBack={() => navigate('/')} />;
  }

  if (path === '/learn' || path.startsWith('/learn/')) {
    const conceptId = path.startsWith('/learn/') ? decodeURIComponent(path.slice(7)) : null;
    return (
      <Learn
        conceptId={conceptId}
        // An empty id is the index; anything else is that article.
        onOpen={(id) => navigate(id ? `/learn/${encodeURIComponent(id)}` : '/learn')}
        onBack={() => navigate('/')}
        onOpenScenario={openScenario}
        onSignIn={() => navigate('/login')}
      />
    );
  }

  if (path === '/discuss' || path.startsWith('/discuss/')) {
    // Non-numeric ids cannot exist, so anything unparseable is the index rather than a
    // request that would 404 on the way to the same place.
    const postId = Number(path.slice(9));
    return (
      <Discuss
        postId={Number.isInteger(postId) && postId > 0 ? postId : null}
        onOpen={(id) => navigate(id === null ? '/discuss' : `/discuss/${id}`)}
        onBack={() => navigate('/')}
        onOpenScenario={openScenario}
        onSignIn={() => navigate('/login')}
      />
    );
  }

  return scenarioId ? (
    <Workspace scenarioId={scenarioId} onBack={() => navigate('/')} />
  ) : (
    <ProblemList
      onOpen={openScenario}
      onSignIn={() => navigate('/login')}
      onLearn={() => navigate('/learn')}
      onDiscuss={() => navigate('/discuss')}
    />
  );
}

export default function App() {
  return (
    <AuthProvider>
      <Routes />
    </AuthProvider>
  );
}
